//! App state, the play queue, and input.

#[path = "prefs.rs"]
pub mod prefs;

use crate::api::{self, Album, Artist, Client, Lyrics, Playlist, SearchResults, Song, StarKind};
use crate::art::{Art, Mode};
use crate::config::Config;
use crate::player::{BANDS, Cmd, EqConfig, Engine, Input, InputSource, Status};
use crate::search::{self, Ranked};
use crate::settings::{Quality, ReplayGain, ShuffleMode, StartPage};
use crate::store::{self, Downloads, Job, SharedProgress};
use crossbeam_channel::{Receiver, Sender, unbounded};
use prefs::{SKey, SPage};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- items

#[derive(Clone, Debug)]
pub enum Item {
    Song(Song),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::Song(s) => &s.id,
            Item::Album(a) => &a.id,
            Item::Artist(a) => &a.id,
            Item::Playlist(p) => &p.id,
        }
    }
    pub fn title(&self) -> &str {
        match self {
            Item::Song(s) => &s.title,
            Item::Album(a) => &a.name,
            Item::Artist(a) => &a.name,
            Item::Playlist(p) => &p.name,
        }
    }
    pub fn kind_label(&self) -> &'static str {
        match self {
            Item::Song(_) => "Song",
            Item::Album(_) => "Album",
            Item::Artist(_) => "Artist",
            Item::Playlist(_) => "Playlist",
        }
    }
    pub fn subtitle(&self) -> String {
        let base = match self {
            Item::Song(s) => s.artist.clone(),
            Item::Album(a) => match a.year {
                Some(y) if y > 0 => format!("{y} · {}", a.artist),
                _ => a.artist.clone(),
            },
            Item::Artist(a) if a.album_count > 0 => {
                format!("{} album{}", a.album_count, if a.album_count == 1 { "" } else { "s" })
            }
            Item::Artist(_) => "Artist".into(),
            Item::Playlist(p) => format!("{} songs", p.song_count),
        };
        match api::provider_name(self.id()) {
            Some(p) => format!("{p} · {base}"),
            None => base,
        }
    }
    pub fn cover(&self) -> Option<&str> {
        match self {
            Item::Song(s) => s.cover.as_deref(),
            Item::Album(a) => a.cover.as_deref(),
            Item::Artist(a) => a.cover.as_deref(),
            Item::Playlist(p) => p.cover.as_deref(),
        }
    }
}

// ---------------------------------------------------------------- views

#[derive(Default)]
pub struct Grid {
    pub items: Vec<Item>,
    pub sel: usize,
    pub scroll: usize,
    pub cols: usize,
    pub rows_visible: usize,
}

impl Grid {
    pub fn new(items: Vec<Item>) -> Self {
        Self { items, cols: 1, rows_visible: 1, ..Default::default() }
    }
    /// Returns false when the move would leave the grid.
    pub fn step(&mut self, dx: i32, dy: i32) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let cols = self.cols.max(1) as i32;
        let i = self.sel as i32;
        if dx != 0 {
            let col = i % cols;
            if (dx < 0 && col == 0) || (dx > 0 && (col == cols - 1 || i + 1 >= self.items.len() as i32)) {
                return false;
            }
            self.sel = (i + dx) as usize;
            return true;
        }
        let next = i + dy * cols;
        if next < 0 {
            return false;
        }
        if next >= self.items.len() as i32 {
            let last_row = (self.items.len() as i32 - 1) / cols;
            if i / cols < last_row {
                self.sel = self.items.len() - 1;
                return true;
            }
            return false;
        }
        self.sel = next as usize;
        true
    }
    pub fn page(&mut self, dir: i32) {
        let n = (self.rows_visible.max(1) as i32) * dir;
        for _ in 0..n.abs() {
            if !self.step(0, n.signum()) {
                break;
            }
        }
    }
    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.sel)
    }
}

pub struct Shelf {
    pub title: String,
    pub items: Vec<Item>,
    pub sel: usize,
    pub offset: usize,
}

#[derive(Clone)]
pub enum Source {
    Album(String),
    Playlist(String),
    Liked,
    Downloads,
    /// A fixed list, such as every song a search found.
    Songs,
}

pub struct Header {
    pub kind: &'static str,
    pub title: String,
    pub subtitle: String,
    pub cover: Option<String>,
    pub seed: String,
    pub liked: bool,
    pub star: Option<(String, StarKind, bool)>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AlbumSort {
    Name,
    Newest,
    Recent,
    Frequent,
    Random,
}

impl AlbumSort {
    pub fn api(&self) -> &'static str {
        match self {
            AlbumSort::Name => "alphabeticalByName",
            AlbumSort::Newest => "newest",
            AlbumSort::Recent => "recent",
            AlbumSort::Frequent => "frequent",
            AlbumSort::Random => "random",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            AlbumSort::Name => "A to Z",
            AlbumSort::Newest => "Recently added",
            AlbumSort::Recent => "Recently played",
            AlbumSort::Frequent => "Most played",
            AlbumSort::Random => "Random",
        }
    }
    pub const ALL: [AlbumSort; 5] =
        [AlbumSort::Name, AlbumSort::Newest, AlbumSort::Recent, AlbumSort::Frequent, AlbumSort::Random];
}

#[derive(Clone, Copy, PartialEq)]
pub enum GridKind {
    Albums(AlbumSort),
    Artists,
    Playlists,
}

/// Search shows the songs list this long before "See all".
pub const SEARCH_SONGS: usize = 5;

/// Search sections, in the order they sit on the page.
pub const SEC_TOP: usize = 0;
pub const SEC_SONGS: usize = 1;
pub const SEC_ARTISTS: usize = 2;
pub const SEC_ALBUMS: usize = 3;
pub const SEC_PLAYLISTS: usize = 4;

pub struct SearchState {
    pub query: String,
    pub editing: bool,
    pub ranked: Option<Ranked>,
    pub sec: usize,
    pub song_sel: usize,
    /// (selected, first visible) for the artists, albums and playlists shelves.
    pub shelf: [(usize, usize); 3],
    pub recent_sel: usize,
    pub searched: String,
    pub last_edit: Instant,
    pub busy: bool,
}

impl SearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            editing: true,
            ranked: None,
            sec: SEC_TOP,
            song_sel: 0,
            shelf: [(0, 0); 3],
            recent_sel: 0,
            searched: String::new(),
            last_edit: Instant::now(),
            busy: false,
        }
    }

    pub fn has(&self, sec: usize) -> bool {
        let Some(r) = &self.ranked else { return false };
        match sec {
            SEC_TOP => r.top.is_some(),
            SEC_SONGS => !r.songs.is_empty(),
            SEC_ARTISTS => !r.artists.is_empty(),
            SEC_ALBUMS => !r.albums.is_empty(),
            SEC_PLAYLISTS => !r.playlists.is_empty(),
            _ => false,
        }
    }

    pub fn song_rows(&self) -> usize {
        let n = self.ranked.as_ref().map(|r| r.songs.len()).unwrap_or(0);
        n.min(SEARCH_SONGS) + usize::from(n > SEARCH_SONGS)
    }

    pub fn shelf_items(&self, sec: usize) -> Vec<Item> {
        let Some(r) = &self.ranked else { return Vec::new() };
        match sec {
            SEC_ARTISTS => r.artists.iter().cloned().map(Item::Artist).collect(),
            SEC_ALBUMS => r.albums.iter().cloned().map(Item::Album).collect(),
            SEC_PLAYLISTS => r.playlists.iter().cloned().map(Item::Playlist).collect(),
            _ => Vec::new(),
        }
    }

    pub fn shelf_len(&self, sec: usize) -> usize {
        let Some(r) = &self.ranked else { return 0 };
        match sec {
            SEC_ARTISTS => r.artists.len(),
            SEC_ALBUMS => r.albums.len(),
            SEC_PLAYLISTS => r.playlists.len(),
            _ => 0,
        }
    }

    fn first(&self) -> Option<usize> {
        (SEC_TOP..=SEC_PLAYLISTS).find(|s| self.has(*s))
    }

    fn after(&self, sec: usize) -> Option<usize> {
        // Top and Songs sit side by side, so down from either goes below both.
        let from = if sec <= SEC_SONGS { SEC_SONGS } else { sec };
        (from + 1..=SEC_PLAYLISTS).find(|s| self.has(*s))
    }

    fn before(&self, sec: usize) -> Option<usize> {
        if sec <= SEC_SONGS {
            return None;
        }
        (SEC_ARTISTS..sec).rev().find(|s| self.has(*s)).or_else(|| {
            if self.has(SEC_TOP) {
                Some(SEC_TOP)
            } else if self.has(SEC_SONGS) {
                Some(SEC_SONGS)
            } else {
                None
            }
        })
    }
}

pub enum Kind {
    Home { shelves: Vec<Shelf>, row: usize, scroll: usize },
    Search(Box<SearchState>),
    Grid { title: String, kind: GridKind, grid: Grid, exhausted: bool, fetching: bool },
    Tracks { header: Option<Header>, source: Source, songs: Vec<Song>, sel: usize, scroll: usize },
    Artist { artist: Option<Artist>, top: Vec<Song>, top_sel: usize, albums: Grid, section: usize },
    Queue { sel: usize, scroll: usize },
    NowPlaying,
    Settings { page: SPage, sel: usize, scroll: usize },
}

pub struct View {
    pub uid: u64,
    pub kind: Kind,
    pub title: String,
    pub loading: bool,
    pub error: Option<String>,
}

pub const NAV: [(&str, &str); 9] = [
    ("⌂", "Home"),
    ("⌕", "Search"),
    ("◫", "Albums"),
    ("♫", "Artists"),
    ("≡", "Playlists"),
    ("♥", "Liked Songs"),
    ("↓", "Downloads"),
    ("≣", "Queue"),
    ("⚙", "Settings"),
];
pub const NAV_DOWNLOADS: usize = 6;
pub const NAV_QUEUE: usize = 7;
pub const NAV_SETTINGS: usize = 8;

// ---------------------------------------------------------------- overlays

#[derive(Clone)]
pub enum Collection {
    Album(String),
    Playlist(String),
    ArtistTop(String),
    Radio(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum PlayMode {
    Play,
    Shuffle,
    Next,
    Queue,
    Download,
}

#[derive(Clone)]
pub enum Action {
    Play(Vec<Song>, usize, String),
    PlayNext(Vec<Song>),
    Queue(Vec<Song>),
    Fetch(Collection, PlayMode, String),
    OpenAlbum(String),
    OpenArtist(String),
    OpenPlaylist(String),
    Star(String, StarKind, bool),
    PickPlaylist(Vec<Song>),
    AddToPlaylist(String, String, Vec<Song>),
    NewPlaylist(Vec<Song>),
    Download(Vec<Song>),
    RemoveDownload(Vec<Song>),
    QueueJump(usize),
    QueueRemove(usize),
    QueueMoveNext(usize),
    QueueMoveEnd(usize),
    QueueClearUpcoming,
    QueueShuffleUpcoming,
    SaveQueue,
    SignOut,
}

pub struct Menu {
    pub title: String,
    pub entries: Vec<(String, Action)>,
    pub sel: usize,
    pub anchor: Option<(u16, u16)>,
}

#[derive(Clone)]
pub enum PromptFor {
    SaveQueue,
    NewPlaylist(Vec<Song>),
}

pub struct Prompt {
    pub title: String,
    pub text: String,
    pub purpose: PromptFor,
}

pub enum Overlay {
    Help,
    Menu(Menu),
    Prompt(Prompt),
}

// ---------------------------------------------------------------- hits

#[derive(Clone, Copy, Debug)]
pub enum Hit {
    Nav(usize),
    Shelf(usize, usize),
    GridItem(usize),
    Track(usize),
    TopSong(usize),
    ArtistAlbum(usize),
    QueueRow(usize),
    SearchBox,
    SearchTop,
    SearchSong(usize),
    SearchSeeAll,
    SearchShelf(usize, usize),
    Recent(usize),
    SortChip(usize),
    HeaderPlay,
    HeaderShuffle,
    HeaderStar,
    HeaderDownload,
    QueueShuffle,
    QueueClear,
    QueueSave,
    SettingRow(usize),
    SettingSlider(usize),
    Prev,
    Toggle,
    Next,
    Shuffle,
    Repeat,
    LikeCurrent,
    Seek,
    Volume,
    OpenNowPlaying,
    OpenQueue,
    Back,
    Crumb(usize),
    MenuItem(usize),
    Outside,
    LoginField(usize),
    LoginSubmit,
}

// ---------------------------------------------------------------- messages

pub enum Msg {
    Home(u64, Vec<Shelf>),
    Grid(u64, Vec<Item>, bool),
    Tracks(u64, Option<Header>, Vec<Song>),
    Artist(u64, Artist, Vec<Song>, Vec<Album>),
    Search(u64, String, SearchResults),
    Error(u64, String),
    Toast(String),
    ToastErr(String),
    Lyrics(String, Option<Lyrics>),
    Songs(PlayMode, Vec<Song>, String),
    Playlists(Vec<Playlist>, Vec<Song>),
    MyPlaylists(Vec<Playlist>),
    Resume(Vec<Song>, usize, f64),
    Stream(u64, Result<Input, String>, f64, bool, f32),
    Preloaded(u64, Result<Input, String>, f32),
    Starred(String, bool),
    Login(Result<(Client, Config), String>),
    Downloaded(store::Download),
    DownloadFailed(String, String, String),
    Prefetched(String),
    Autoplay(Vec<Song>),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Repeat {
    Off,
    All,
    One,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Drag {
    None,
    Seek,
    Volume,
    Queue(usize),
    Slider(usize),
}

pub struct LoginForm {
    pub fields: [String; 3],
    pub field: usize,
    pub busy: bool,
    pub error: Option<String>,
}

pub enum Screen {
    Login(LoginForm),
    Main,
}

/// Where a song plays from.
enum Plan {
    File(PathBuf),
    Stream { url: String, id: String, hint: Option<String>, ext: String },
    Offline,
}

pub struct App {
    pub cfg: Config,
    pub screen: Screen,
    pub client: Option<Client>,
    pub art: Option<Art>,
    picker: Picker,
    engine: Option<Engine>,
    pub engine_error: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub stack: Vec<View>,
    next_uid: u64,

    pub queue: Vec<Song>,
    pub qi: usize,
    original: Option<Vec<Song>>,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub current: Option<Song>,
    pub context: String,
    generation: u64,
    gen_seq: u64,
    preload: Option<(u64, usize, String)>,
    transcoded: bool,
    from_file: bool,
    pub status: Status,
    scrobbled: bool,
    pub volume: f32,
    pub lyrics: Option<(String, Option<Lyrics>)>,
    history: Vec<String>,
    pub spectrum: [f32; BANDS],

    pub downloads: Downloads,
    pub downloaded_ids: HashSet<String>,
    dl_tx: Option<Sender<Job>>,
    pub dl_progress: SharedProgress,
    dl_pending: HashSet<String>,
    prefetching: Option<String>,
    last_prefetch: Instant,
    pub my_playlists: Vec<Playlist>,
    pub confirm: Option<(SKey, Instant)>,

    pub overlay: Option<Overlay>,
    pub toast: Option<(String, Instant, bool)>,
    pub hits: Vec<(Rect, Hit)>,
    pub seek_rect: Rect,
    pub volume_rect: Rect,
    pub slider_rects: Vec<(usize, Rect)>,
    /// Where the progress bar is being dragged to, before letting go.
    pub seek_preview: Option<f64>,
    stall_since: Option<Instant>,
    stall_retries: u32,
    last_click: Option<(Instant, u16, u16)>,
    drag: Drag,
    pub quit: bool,
    pub greeting: String,
    pub dirty: bool,
}

pub fn fmt_time(secs: f64) -> String {
    let t = secs.max(0.0) as u64;
    if t >= 3600 {
        format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
    } else {
        format!("{}:{:02}", t / 60, t % 60)
    }
}

pub fn fmt_long(secs: u32) -> String {
    let m = secs / 60;
    if m >= 60 { format!("{} hr {} min", m / 60, m % 60) } else { format!("{m} min") }
}

pub fn songs_label(n: usize) -> String {
    if n == 1 { "1 song".into() } else { format!("{n} songs") }
}

pub fn fmt_bytes(b: u64) -> String {
    let mb = b as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") }
}

fn rand_u64() -> u64 {
    let mut x: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn shuffle_vec<T>(v: &mut [T]) {
    let mut x = rand_u64();
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
}

/// Formats Symphonia decodes. Anything else, Opus included, gets transcoded.
fn decodable(song: &Song) -> bool {
    matches!(
        song.suffix.as_deref().map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("mp3" | "flac" | "m4a" | "mp4" | "aac" | "ogg" | "oga" | "wav" | "aif" | "aiff" | "alac" | "caf")
    )
}

fn lossless(song: &Song) -> bool {
    matches!(
        song.suffix.as_deref().map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("flac" | "alac" | "wav" | "aif" | "aiff" | "ape" | "wv" | "dsf" | "dff")
    )
}

impl App {
    pub fn new(cfg: Config, picker: Picker) -> Self {
        let (tx, rx) = unbounded();
        let greeting = greeting();
        let downloads = Downloads::load();
        let downloaded_ids = downloads.items.iter().map(|d| d.id.clone()).collect();
        let mut app = Self {
            volume: cfg.volume,
            cfg,
            screen: Screen::Main,
            client: None,
            art: None,
            picker,
            engine: None,
            engine_error: None,
            tx,
            rx,
            stack: Vec::new(),
            next_uid: 1,
            queue: Vec::new(),
            qi: 0,
            original: None,
            shuffle: false,
            repeat: Repeat::Off,
            current: None,
            context: String::new(),
            generation: 0,
            gen_seq: 0,
            preload: None,
            transcoded: false,
            from_file: false,
            status: Status { paused: true, ..Default::default() },
            scrobbled: false,
            lyrics: None,
            history: Vec::new(),
            spectrum: [0.0; BANDS],
            downloads,
            downloaded_ids,
            dl_tx: None,
            dl_progress: Default::default(),
            dl_pending: HashSet::new(),
            prefetching: None,
            last_prefetch: Instant::now(),
            my_playlists: Vec::new(),
            confirm: None,
            overlay: None,
            toast: None,
            hits: Vec::new(),
            seek_rect: Rect::default(),
            volume_rect: Rect::default(),
            slider_rects: Vec::new(),
            seek_preview: None,
            stall_since: None,
            stall_retries: 0,
            last_click: None,
            drag: Drag::None,
            quit: false,
            greeting,
            dirty: true,
        };
        match Engine::start(app.volume * app.volume) {
            Ok(e) => app.engine = Some(e),
            Err(e) => app.engine_error = Some(e),
        }
        app.apply_settings();
        if app.cfg.signed_in() {
            let c = Client::new(&app.cfg.server, &app.cfg.username, &app.cfg.salt, &app.cfg.token);
            app.enter_main(c);
        } else {
            app.screen = Screen::Login(LoginForm {
                fields: [app.cfg.server.clone(), app.cfg.username.clone(), String::new()],
                field: if app.cfg.server.is_empty() { 0 } else { 2 },
                busy: false,
                error: None,
            });
        }
        app
    }

    fn enter_main(&mut self, client: Client) {
        crate::log::info(format!("signed in as {} on {}", client.username(), client.server()));
        self.art = Some(Art::new(client.clone(), self.picker.clone(), Mode::from_str(&self.cfg.images)));
        self.client = Some(client.clone());
        self.screen = Screen::Main;
        self.start_download_worker(client.clone());
        let start = match self.cfg.settings.start_page {
            StartPage::Home => 0,
            StartPage::Search => 1,
            StartPage::Albums => 2,
            StartPage::Playlists => 4,
            StartPage::Liked => 5,
            StartPage::Downloads => NAV_DOWNLOADS,
        };
        self.open_nav(start);
        if self.cfg.settings.restore_queue && !self.cfg.settings.offline_mode {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                if let Some((songs, current, pos)) = client.play_queue() {
                    let idx = current.and_then(|c| songs.iter().position(|s| s.id == c)).unwrap_or(0);
                    let _ = tx.send(Msg::Resume(songs, idx, pos));
                }
            });
        }
        self.refresh_my_playlists();
    }

    fn start_download_worker(&mut self, client: Client) {
        let (jtx, jrx) = unbounded::<Job>();
        self.dl_tx = Some(jtx);
        let tx = self.tx.clone();
        let progress = self.dl_progress.clone();
        std::thread::Builder::new()
            .name("downloads".into())
            .spawn(move || {
                while let Ok(job) = jrx.recv() {
                    progress.lock().unwrap().current = Some((job.song.title.clone(), 0, None));
                    let file = format!("{:x}.{}", md5::compute(&job.song.id), job.ext);
                    let dest = store::downloads_dir().join(&file);
                    let r = store::fetch_to(&client, &job.url, &dest, Some(&progress));
                    {
                        let mut p = progress.lock().unwrap();
                        p.current = None;
                        if r.is_ok() {
                            p.done += 1;
                        } else {
                            p.failed += 1;
                        }
                    }
                    let s = job.song;
                    let msg = match r {
                        Ok(bytes) => Msg::Downloaded(store::Download {
                            id: s.id.clone(),
                            title: s.title.clone(),
                            artist: s.artist.clone(),
                            artist_id: s.artist_id.clone(),
                            album: s.album.clone(),
                            album_id: s.album_id.clone(),
                            cover: s.cover.clone(),
                            duration: s.duration,
                            track: s.track,
                            year: s.year,
                            suffix: job.ext.clone(),
                            bytes,
                            file,
                            quality: job.quality,
                        }),
                        Err(e) => Msg::DownloadFailed(s.id, s.title, e),
                    };
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            })
            .ok();
    }

    pub fn shutdown(&mut self) {
        self.cfg.volume = self.volume;
        if let Some(a) = &self.art {
            self.cfg.images = a.mode.as_str().into();
        }
        if self.client.is_some() {
            self.cfg.save();
        }
        if let (Some(c), Some(cur), false) = (&self.client, &self.current, self.cfg.settings.offline_mode) {
            let ids: Vec<String> = self.queue.iter().map(|s| s.id.clone()).collect();
            c.save_play_queue(&ids, Some(&cur.id), self.status.position);
        }
        if let Some(e) = &self.engine {
            e.send(Cmd::Stop);
        }
        crate::log::info("quit");
    }

    /// Pushes every setting that lives outside the config into effect.
    pub fn apply_settings(&mut self) {
        let s = &self.cfg.settings;
        crate::theme::set_accent(s.accent.rgb());
        store::set_cache_limit_mb(s.cache_limit_mb);
        if let Some(e) = &self.engine {
            e.send(Cmd::Eq(EqConfig { enabled: s.eq_enabled, preamp_db: s.eq_preamp, gains: s.eq_gains }));
            e.send(Cmd::Transitions { crossfade: s.crossfade as f32, gapless: s.gapless });
        }
        self.dirty = true;
    }

    // ------------------------------------------------------------ async

    fn spawn<F>(&self, f: F)
    where
        F: FnOnce(&Client) -> Option<Msg> + Send + 'static,
    {
        let Some(c) = self.client.clone() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Some(m) = f(&c) {
                let _ = tx.send(m);
            }
        });
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now(), false));
        self.dirty = true;
    }

    pub fn toast_err(&mut self, s: impl Into<String>) {
        let s = s.into();
        crate::log::warn(format!("shown: {s}"));
        self.toast = Some((s, Instant::now(), true));
        self.dirty = true;
    }

    fn uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    fn view_mut(&mut self, uid: u64) -> Option<&mut View> {
        self.stack.iter_mut().find(|v| v.uid == uid)
    }

    pub fn top(&self) -> Option<&View> {
        self.stack.last()
    }

    fn top_kind(&self) -> Option<&Kind> {
        self.stack.last().map(|v| &v.kind)
    }

    fn search_mut(&mut self) -> Option<&mut SearchState> {
        match self.stack.last_mut().map(|v| &mut v.kind) {
            Some(Kind::Search(s)) => Some(s),
            _ => None,
        }
    }

    fn refresh_my_playlists(&self) {
        self.spawn(|c| c.playlists().ok().map(Msg::MyPlaylists));
    }

    // ------------------------------------------------------------ navigation

    pub fn open_nav(&mut self, i: usize) {
        self.overlay = None;
        let uid = self.uid();
        let kind = match i {
            0 => Kind::Home { shelves: Vec::new(), row: 0, scroll: 0 },
            1 => {
                self.refresh_my_playlists();
                Kind::Search(Box::new(SearchState::new()))
            }
            2 => Kind::Grid {
                title: "Albums".into(),
                kind: GridKind::Albums(AlbumSort::Name),
                grid: Grid::new(Vec::new()),
                exhausted: false,
                fetching: true,
            },
            3 => Kind::Grid {
                title: "Artists".into(),
                kind: GridKind::Artists,
                grid: Grid::new(Vec::new()),
                exhausted: true,
                fetching: true,
            },
            4 => Kind::Grid {
                title: "Playlists".into(),
                kind: GridKind::Playlists,
                grid: Grid::new(Vec::new()),
                exhausted: true,
                fetching: true,
            },
            5 => Kind::Tracks { header: None, source: Source::Liked, songs: Vec::new(), sel: 0, scroll: 0 },
            NAV_DOWNLOADS => Kind::Tracks { header: None, source: Source::Downloads, songs: Vec::new(), sel: 0, scroll: 0 },
            NAV_QUEUE => Kind::Queue { sel: self.qi, scroll: 0 },
            _ => Kind::Settings { page: SPage::Root, sel: 0, scroll: 0 },
        };
        let loading = !matches!(kind, Kind::Search(_) | Kind::Queue { .. } | Kind::Settings { .. });
        let title = NAV.get(i).map(|n| n.1).unwrap_or("Settings").to_string();
        self.stack = vec![View { uid, kind, title, loading, error: None }];
        self.load(uid);
        self.dirty = true;
    }

    fn push(&mut self, kind: Kind, title: impl Into<String>) -> u64 {
        let uid = self.uid();
        let loading = !matches!(kind, Kind::NowPlaying | Kind::Queue { .. } | Kind::Settings { .. });
        self.stack.push(View { uid, kind, title: title.into(), loading, error: None });
        self.dirty = true;
        uid
    }

    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
        self.dirty = true;
    }

    /// Goes back to the start of the given depth (0 is the sidebar page).
    pub fn back_to(&mut self, depth: usize) {
        self.stack.truncate(depth + 1);
        self.dirty = true;
    }

    pub fn open_album(&mut self, id: String) {
        let uid = self.push(
            Kind::Tracks { header: None, source: Source::Album(id), songs: Vec::new(), sel: 0, scroll: 0 },
            "Album",
        );
        self.load(uid);
    }

    pub fn open_playlist(&mut self, id: String) {
        let uid = self.push(
            Kind::Tracks { header: None, source: Source::Playlist(id), songs: Vec::new(), sel: 0, scroll: 0 },
            "Playlist",
        );
        self.load(uid);
    }

    pub fn open_artist(&mut self, id: String) {
        let uid = self.push(
            Kind::Artist {
                artist: Some(Artist { id, ..Default::default() }),
                top: Vec::new(),
                top_sel: 0,
                albums: Grid::new(Vec::new()),
                section: 0,
            },
            "Artist",
        );
        self.load(uid);
    }

    pub fn open_songs(&mut self, title: String, songs: Vec<Song>) {
        let total: u32 = songs.iter().map(|s| s.duration).sum();
        let header = Header {
            kind: "Songs",
            title: title.clone(),
            subtitle: format!("{}, {}", songs_label(songs.len()), fmt_long(total)),
            cover: None,
            seed: title.clone(),
            liked: false,
            star: None,
        };
        let uid = self.push(
            Kind::Tracks { header: Some(header), source: Source::Songs, songs, sel: 0, scroll: 0 },
            title,
        );
        if let Some(v) = self.view_mut(uid) {
            v.loading = false;
        }
    }

    pub fn open_settings(&mut self, page: SPage) {
        if page == SPage::Root {
            self.open_nav(NAV_SETTINGS);
            return;
        }
        self.push(Kind::Settings { page, sel: 0, scroll: 0 }, page.title());
    }

    pub fn open_now_playing(&mut self) {
        if matches!(self.top_kind(), Some(Kind::NowPlaying)) {
            self.back();
            return;
        }
        if self.current.is_none() {
            self.toast("Nothing is playing yet.");
            return;
        }
        self.push(Kind::NowPlaying, "Now Playing");
    }

    fn downloads_header(&self) -> Header {
        let p = self.dl_progress.lock().unwrap();
        let mut sub = format!("{} · {}", songs_label(self.downloads.items.len()), fmt_bytes(self.downloads.bytes()));
        if let Some((t, have, total)) = &p.current {
            let pct = total.map(|t| format!(" {}%", have * 100 / t.max(1))).unwrap_or_default();
            sub.push_str(&format!(" · downloading {t}{pct}"));
            let left = p.queued.saturating_sub(p.done + p.failed + 1);
            if left > 0 {
                sub.push_str(&format!(", {left} more waiting"));
            }
        }
        Header {
            kind: "Playlist",
            title: "Downloads".into(),
            subtitle: sub,
            cover: None,
            seed: "downloads".into(),
            liked: false,
            star: None,
        }
    }

    fn load(&mut self, uid: u64) {
        let hide = self.cfg.settings.hide_explicit;
        let Some(view) = self.stack.iter().find(|v| v.uid == uid) else { return };
        match &view.kind {
            Kind::Home { .. } => self.spawn(move |c| {
                let mut shelves = Vec::new();
                let mut first_err = None;
                for (title, kind) in [
                    ("Recently played", "recent"),
                    ("Recently added", "newest"),
                    ("Most played", "frequent"),
                    ("Something different", "random"),
                ] {
                    match c.album_list(kind, 24, 0) {
                        Ok(list) if !list.is_empty() => shelves.push(Shelf {
                            title: title.into(),
                            items: list.into_iter().map(Item::Album).collect(),
                            sel: 0,
                            offset: 0,
                        }),
                        Ok(_) => {}
                        Err(e) => {
                            first_err.get_or_insert(e);
                        }
                    }
                }
                if let Ok(p) = c.playlists() {
                    if !p.is_empty() {
                        shelves.insert(
                            1.min(shelves.len()),
                            Shelf {
                                title: "Your playlists".into(),
                                items: p.into_iter().map(Item::Playlist).collect(),
                                sel: 0,
                                offset: 0,
                            },
                        );
                    }
                }
                if shelves.is_empty() {
                    if let Some(e) = first_err {
                        return Some(Msg::Error(uid, e));
                    }
                }
                Some(Msg::Home(uid, shelves))
            }),
            Kind::Grid { kind, grid, .. } => {
                let kind = *kind;
                let offset = grid.items.len() as u32;
                self.spawn(move |c| {
                    let r = match kind {
                        GridKind::Albums(sort) => c
                            .album_list(sort.api(), 120, offset)
                            .map(|v| v.into_iter().map(Item::Album).collect::<Vec<_>>()),
                        GridKind::Artists => c.artists().map(|v| v.into_iter().map(Item::Artist).collect()),
                        GridKind::Playlists => c.playlists().map(|v| v.into_iter().map(Item::Playlist).collect()),
                    };
                    Some(match r {
                        Ok(items) => Msg::Grid(uid, items, offset > 0),
                        Err(e) => Msg::Error(uid, e),
                    })
                })
            }
            Kind::Tracks { source: Source::Downloads, .. } => {
                let songs = self.downloads.songs();
                let header = self.downloads_header();
                let _ = self.tx.send(Msg::Tracks(uid, Some(header), songs));
            }
            Kind::Tracks { source: Source::Songs, .. } => {}
            Kind::Tracks { source, .. } => {
                let source = source.clone();
                self.spawn(move |c| {
                    let keep = |v: Vec<Song>| -> Vec<Song> { v.into_iter().filter(|s| !(hide && s.explicit)).collect() };
                    Some(match source {
                        Source::Album(id) => match c.album(&id) {
                            Ok((a, songs)) => {
                                let songs = keep(songs);
                                let total: u32 = songs.iter().map(|s| s.duration).sum();
                                let mut sub = vec![a.artist.clone()];
                                if let Some(y) = a.year.filter(|y| *y > 0) {
                                    sub.push(y.to_string());
                                }
                                sub.push(format!("{}, {}", songs_label(songs.len()), fmt_long(total)));
                                if let Some(p) = api::provider_name(&a.id) {
                                    sub.push(format!("via {p}"));
                                }
                                Msg::Tracks(
                                    uid,
                                    Some(Header {
                                        kind: "Album",
                                        title: a.name.clone(),
                                        subtitle: sub.join(" · "),
                                        cover: a.cover.clone(),
                                        seed: a.id.clone(),
                                        liked: false,
                                        star: Some((a.id.clone(), StarKind::Album, a.starred)),
                                    }),
                                    songs,
                                )
                            }
                            Err(e) => Msg::Error(uid, e),
                        },
                        Source::Playlist(id) => match c.playlist(&id) {
                            Ok((p, songs)) => {
                                let songs = keep(songs);
                                let total: u32 = songs.iter().map(|s| s.duration).sum();
                                let mut sub = Vec::new();
                                if !p.owner.is_empty() {
                                    sub.push(p.owner.clone());
                                }
                                sub.push(format!("{}, {}", songs_label(songs.len()), fmt_long(total)));
                                Msg::Tracks(
                                    uid,
                                    Some(Header {
                                        kind: "Playlist",
                                        title: p.name.clone(),
                                        subtitle: sub.join(" · "),
                                        cover: p.cover.clone(),
                                        seed: p.id.clone(),
                                        liked: false,
                                        star: None,
                                    }),
                                    songs,
                                )
                            }
                            Err(e) => Msg::Error(uid, e),
                        },
                        Source::Liked => match c.starred() {
                            Ok(r) => {
                                let songs = keep(r.songs);
                                let total: u32 = songs.iter().map(|s| s.duration).sum();
                                Msg::Tracks(
                                    uid,
                                    Some(Header {
                                        kind: "Playlist",
                                        title: "Liked Songs".into(),
                                        subtitle: format!("{} · {}, {}", c.username(), songs_label(songs.len()), fmt_long(total)),
                                        cover: None,
                                        seed: "liked".into(),
                                        liked: true,
                                        star: None,
                                    }),
                                    songs,
                                )
                            }
                            Err(e) => Msg::Error(uid, e),
                        },
                        Source::Downloads | Source::Songs => return None,
                    })
                })
            }
            Kind::Artist { artist, .. } => {
                let id = artist.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                self.spawn(move |c| {
                    Some(match c.artist(&id) {
                        Ok((a, albums)) => {
                            let top = c.top_songs(&a.name, 10).unwrap_or_default();
                            let top = top.into_iter().filter(|s| !(hide && s.explicit)).collect();
                            Msg::Artist(uid, a, top, albums)
                        }
                        Err(e) => Msg::Error(uid, e),
                    })
                })
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ messages

    pub fn pump(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.dirty = true;
            self.on_msg(msg);
        }
        if let Some(art) = &mut self.art {
            if art.poll() {
                self.dirty = true;
            }
        }
        if let Some((_, at, err)) = &self.toast {
            let ttl = if *err { 4200 } else { 2600 };
            if at.elapsed() > Duration::from_millis(ttl) {
                self.toast = None;
                self.dirty = true;
            }
        }
        if let Some((_, at)) = self.confirm {
            if at.elapsed() > Duration::from_secs(4) {
                self.confirm = None;
                self.dirty = true;
            }
        }
        self.tick_search();
        self.tick_player();
        self.tick_prefetch();
        if let Some(e) = &self.engine {
            let s = e.spectrum();
            if self.viz_visible() && s.iter().zip(self.spectrum.iter()).any(|(a, b)| (a - b).abs() > 0.01) {
                self.dirty = true;
            }
            self.spectrum = s;
        }
    }

    /// Whether anything on screen draws the visualizer right now.
    pub fn viz_visible(&self) -> bool {
        let s = &self.cfg.settings;
        let np = matches!(self.top_kind(), Some(Kind::NowPlaying)) && s.visualizer != crate::settings::Visualizer::Off;
        let eq = matches!(self.top_kind(), Some(Kind::Settings { page: SPage::Equalizer, .. }));
        self.current.is_some() && (np || eq || s.bar_visualizer)
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Home(uid, list) => {
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    if let Kind::Home { shelves, .. } = &mut v.kind {
                        *shelves = list;
                    }
                }
            }
            Msg::Grid(uid, items, append) => {
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    if let Kind::Grid { grid, exhausted, fetching, kind, .. } = &mut v.kind {
                        *fetching = false;
                        if matches!(kind, GridKind::Albums(_)) && items.is_empty() {
                            *exhausted = true;
                        }
                        if append {
                            grid.items.extend(items);
                        } else {
                            *grid = Grid::new(items);
                        }
                    }
                }
            }
            Msg::Tracks(uid, h, list) => {
                let playing = self.current.as_ref().map(|s| s.id.clone());
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    if let Some(h) = &h {
                        v.title = h.title.clone();
                    }
                    if let Kind::Tracks { header, songs, sel, .. } = &mut v.kind {
                        *header = h;
                        let keep = songs.get(*sel).map(|s| s.id.clone());
                        *sel = keep
                            .or(playing)
                            .and_then(|p| list.iter().position(|s| s.id == p))
                            .unwrap_or(0);
                        *songs = list;
                    }
                }
            }
            Msg::Artist(uid, a, t, albums) => {
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    v.title = a.name.clone();
                    if let Kind::Artist { artist, top, albums: grid, section, .. } = &mut v.kind {
                        *artist = Some(a);
                        *section = if t.is_empty() { 1 } else { 0 };
                        *top = t;
                        *grid = Grid::new(albums.into_iter().map(Item::Album).collect());
                    }
                }
            }
            Msg::Search(uid, q, r) => {
                let hide = self.cfg.settings.hide_explicit;
                let pls = self.my_playlists.clone();
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    v.error = None;
                    if let Kind::Search(s) = &mut v.kind {
                        // A slower, older search must not replace a newer one.
                        if s.searched != q {
                            return;
                        }
                        s.busy = false;
                        s.ranked = Some(search::rank(&q, r, &pls, hide));
                        s.song_sel = 0;
                        s.shelf = [(0, 0); 3];
                        s.sec = s.first().unwrap_or(SEC_TOP);
                    }
                }
            }
            Msg::Error(uid, e) => {
                crate::log::warn(format!("page failed: {e}"));
                if let Some(v) = self.view_mut(uid) {
                    v.loading = false;
                    v.error = Some(e);
                    match &mut v.kind {
                        Kind::Search(s) => s.busy = false,
                        Kind::Grid { fetching, exhausted, .. } => {
                            *fetching = false;
                            *exhausted = true;
                        }
                        _ => {}
                    }
                }
            }
            Msg::Toast(s) => self.toast(s),
            Msg::ToastErr(s) => self.toast_err(s),
            Msg::Lyrics(id, l) => {
                if self.current.as_ref().is_some_and(|c| c.id == id) {
                    self.lyrics = Some((id, l));
                }
            }
            Msg::Songs(mode, songs, label) => {
                let hide = self.cfg.settings.hide_explicit;
                let songs: Vec<Song> = songs.into_iter().filter(|s| !(hide && s.explicit)).collect();
                if songs.is_empty() {
                    self.toast_err("No songs to play there.");
                    return;
                }
                match mode {
                    PlayMode::Play => self.play_list(songs, 0, false, label),
                    PlayMode::Shuffle => self.play_list(songs, 0, true, label),
                    PlayMode::Next => self.play_next(songs, &label),
                    PlayMode::Queue => self.enqueue(songs, &label),
                    PlayMode::Download => self.download(songs, &label),
                }
            }
            Msg::Playlists(list, songs) => {
                self.my_playlists = list.clone();
                let mut entries = vec![("+  New playlist…".to_string(), Action::NewPlaylist(songs.clone()))];
                entries.extend(
                    list.into_iter().map(|p| (format!("   {}", p.name), Action::AddToPlaylist(p.id, p.name, songs.clone()))),
                );
                self.overlay = Some(Overlay::Menu(Menu { title: "Add to playlist".into(), entries, sel: 0, anchor: None }));
            }
            Msg::MyPlaylists(p) => self.my_playlists = p,
            Msg::Resume(songs, idx, pos) => {
                if self.current.is_none() && self.queue.is_empty() {
                    self.queue = songs;
                    self.qi = idx.min(self.queue.len().saturating_sub(1));
                    self.context = "Your last session".into();
                    self.start_current(pos, true);
                }
            }
            Msg::Stream(g, res, start_at, paused, gain) => {
                if g != self.generation {
                    return;
                }
                match res {
                    Ok(input) => {
                        if let Some(e) = &self.engine {
                            e.send(Cmd::Load { input, generation: g, start_at, paused, gain });
                        }
                    }
                    Err(e) => {
                        self.status.loading = false;
                        self.toast_err(e);
                    }
                }
            }
            Msg::Preloaded(g, res, gain) => {
                if self.preload.as_ref().is_none_or(|p| p.0 != g) {
                    return;
                }
                match res {
                    Ok(input) => {
                        if let Some(e) = &self.engine {
                            e.send(Cmd::Preload { input, generation: g, gain });
                        }
                    }
                    Err(e) => crate::log::warn(format!("couldn't line up the next song: {e}")),
                }
            }
            Msg::Starred(id, on) => self.apply_star(&id, on),
            Msg::Login(r) => {
                if let Screen::Login(form) = &mut self.screen {
                    form.busy = false;
                    match r {
                        Ok((client, cfg)) => {
                            let volume = self.cfg.volume;
                            let settings = self.cfg.settings.clone();
                            self.cfg = cfg;
                            self.cfg.volume = volume;
                            self.cfg.settings = settings;
                            self.cfg.save();
                            self.enter_main(client);
                        }
                        Err(e) => {
                            crate::log::warn(format!("sign in failed: {e}"));
                            form.error = Some(e)
                        }
                    }
                }
            }
            Msg::Downloaded(d) => {
                self.dl_pending.remove(&d.id);
                self.downloaded_ids.insert(d.id.clone());
                self.downloads.add(d);
                self.refresh_downloads_view();
                let p = self.dl_progress.lock().unwrap();
                let finished = p.done + p.failed >= p.queued;
                let (done, failed) = (p.done, p.failed);
                drop(p);
                if finished {
                    *self.dl_progress.lock().unwrap() = Default::default();
                    if failed > 0 {
                        self.toast_err(format!("Downloaded {done} songs. {failed} failed, see the log."));
                    } else {
                        self.toast(format!("Downloaded {done} song{}", if done == 1 { "" } else { "s" }));
                    }
                }
            }
            Msg::DownloadFailed(id, title, e) => {
                crate::log::error(format!("download of \"{title}\" failed: {e}"));
                self.dl_pending.remove(&id);
                self.refresh_downloads_view();
            }
            Msg::Prefetched(id) => {
                if self.prefetching.as_deref() == Some(id.as_str()) {
                    self.prefetching = None;
                }
            }
            Msg::Autoplay(songs) => {
                let have: HashSet<String> = self.queue.iter().map(|s| s.id.clone()).collect();
                let fresh: Vec<Song> = songs.into_iter().filter(|s| !have.contains(&s.id)).collect();
                if fresh.is_empty() || self.qi + 1 != self.queue.len() {
                    return;
                }
                let n = fresh.len();
                self.queue.extend(fresh);
                if let Some(o) = &mut self.original {
                    o.extend(self.queue[self.queue.len() - n..].iter().cloned());
                }
                self.qi += 1;
                self.context = "Autoplay".into();
                self.start_current(0.0, false);
                self.toast("Autoplay: more like the last song");
            }
        }
    }

    fn refresh_downloads_view(&mut self) {
        let uid = self.stack.iter().find(|v| matches!(v.kind, Kind::Tracks { source: Source::Downloads, .. })).map(|v| v.uid);
        if let Some(uid) = uid {
            self.load(uid);
        }
    }

    fn apply_star(&mut self, id: &str, on: bool) {
        let touch = |s: &mut Song| {
            if s.id == id {
                s.starred = on;
            }
        };
        self.queue.iter_mut().for_each(touch);
        if let Some(c) = &mut self.current {
            touch(c);
        }
        for v in &mut self.stack {
            match &mut v.kind {
                Kind::Tracks { songs, header, source, .. } => {
                    songs.iter_mut().for_each(touch);
                    if let Some(h) = header {
                        if let Some(st) = &mut h.star {
                            if st.0 == id {
                                st.2 = on;
                            }
                        }
                    }
                    if matches!(source, Source::Liked) && !on {
                        songs.retain(|s| s.id != id);
                    }
                }
                Kind::Artist { top, .. } => top.iter_mut().for_each(touch),
                Kind::Search(s) => {
                    if let Some(r) = &mut s.ranked {
                        r.songs.iter_mut().for_each(touch);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn star(&mut self, id: String, kind: StarKind, on: bool) {
        self.apply_star(&id, on);
        self.toast(if on { "Added to Liked" } else { "Removed from Liked" });
        self.spawn(move |c| match c.star(&id, kind, on) {
            Ok(()) => None,
            Err(e) => {
                crate::log::warn(format!("star failed: {e}"));
                Some(Msg::Starred(id, !on))
            }
        });
    }

    // ------------------------------------------------------------ playback

    pub fn play_list(&mut self, songs: Vec<Song>, idx: usize, shuffle: bool, context: String) {
        if songs.is_empty() {
            return;
        }
        self.queue = songs;
        self.qi = idx.min(self.queue.len() - 1);
        self.original = None;
        self.context = context;
        if shuffle {
            self.shuffle = true;
            self.qi = (rand_u64() % self.queue.len() as u64) as usize;
        }
        if self.shuffle {
            self.apply_shuffle();
        }
        self.start_current(0.0, false);
    }

    fn apply_shuffle(&mut self) {
        self.original = Some(self.queue.clone());
        let cur = self.queue.remove(self.qi);
        shuffle_vec(&mut self.queue);
        if self.cfg.settings.shuffle_mode == ShuffleMode::FewerRepeats {
            // Songs heard recently go to the back, still in shuffled order.
            let recent: HashSet<&String> = self.history.iter().rev().take(200).collect();
            let (fresh, heard): (Vec<Song>, Vec<Song>) = self.queue.drain(..).partition(|s| !recent.contains(&s.id));
            self.queue = fresh;
            self.queue.extend(heard);
        }
        self.queue.insert(0, cur);
        self.qi = 0;
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.queue.is_empty() {
        } else if self.shuffle {
            self.apply_shuffle();
        } else if let Some(orig) = self.original.take() {
            let cur = self.queue.get(self.qi).map(|s| s.id.clone());
            self.queue = orig;
            self.qi = cur.and_then(|c| self.queue.iter().position(|s| s.id == c)).unwrap_or(0);
        }
        self.toast(if self.shuffle { "Shuffle on" } else { "Shuffle off" });
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.toast(match self.repeat {
            Repeat::Off => "Repeat off",
            Repeat::All => "Repeat all",
            Repeat::One => "Repeat one",
        });
    }

    pub fn enqueue(&mut self, songs: Vec<Song>, label: &str) {
        let n = songs.len();
        if let Some(o) = &mut self.original {
            o.extend(songs.iter().cloned());
        }
        let was_empty = self.queue.is_empty();
        self.queue.extend(songs);
        if was_empty {
            self.qi = 0;
            self.context = label.to_string();
            self.start_current(0.0, false);
        }
        self.toast(if label.is_empty() {
            format!("Added {n} song{} to the queue", if n == 1 { "" } else { "s" })
        } else {
            format!("Added {label} to the queue")
        });
    }

    pub fn play_next(&mut self, songs: Vec<Song>, label: &str) {
        if self.queue.is_empty() {
            self.play_list(songs, 0, false, label.to_string());
            return;
        }
        if let Some(o) = &mut self.original {
            o.extend(songs.iter().cloned());
        }
        let at = (self.qi + 1).min(self.queue.len());
        let n = songs.len();
        for (k, s) in songs.into_iter().enumerate() {
            self.queue.insert(at + k, s);
        }
        self.toast(if label.is_empty() {
            format!("{n} song{} will play next", if n == 1 { "" } else { "s" })
        } else {
            format!("{label} will play next")
        });
    }

    /// Track first, album next, or 1.0, from the song's ReplayGain tags.
    fn gain_for(&self, s: &Song) -> f32 {
        let st = &self.cfg.settings;
        let (g, p) = match st.replay_gain {
            ReplayGain::Off => return 1.0,
            ReplayGain::Track => (s.track_gain.or(s.album_gain), s.track_peak.or(s.album_peak)),
            ReplayGain::Album => (s.album_gain.or(s.track_gain), s.album_peak.or(s.track_peak)),
        };
        let Some(g) = g else { return 1.0 };
        let mut lin = 10f32.powf((g + st.replay_gain_preamp) / 20.0);
        if st.prevent_clipping {
            let peak = p.filter(|p| *p > 0.0).unwrap_or(1.0);
            lin = lin.min(1.0 / peak);
        }
        if lin.is_finite() { lin.clamp(0.05, 4.0) } else { 1.0 }
    }

    fn plan(&self, song: &Song, quality: Quality, force_mp3: bool) -> Plan {
        if let Some(p) = self.downloads.file_for(song) {
            return Plan::File(p);
        }
        if !force_mp3 {
            if let Some(p) = store::cached(&song.id) {
                return Plan::File(p);
            }
        }
        if self.cfg.settings.offline_mode {
            return Plan::Offline;
        }
        let Some(c) = &self.client else { return Plan::Offline };
        let kbps = quality.kbps();
        let wants_cap = kbps > 0
            && song.bitrate.is_none_or(|b| b > kbps)
            && (!self.cfg.settings.transcode_lossless_only || lossless(song));
        let (br, fmt) = if force_mp3 || !decodable(song) {
            (if kbps > 0 { kbps } else { 320 }, Some("mp3"))
        } else if wants_cap {
            (kbps, Some("mp3"))
        } else {
            (0, None)
        };
        let url = c.stream_url(&song.id, br, fmt);
        let ext = store::ext_for(song, fmt);
        Plan::Stream { url, id: song.id.clone(), hint: fmt.map(String::from).or_else(|| song.suffix.clone()), ext }
    }

    fn open_plan<F>(&self, plan: Plan, wrap: F)
    where
        F: FnOnce(Result<Input, String>) -> Msg + Send + 'static,
    {
        let client = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = match plan {
                Plan::File(p) => {
                    let hint = p.extension().map(|e| e.to_string_lossy().to_string());
                    Ok(Input { source: InputSource::File(p), hint })
                }
                Plan::Stream { url, id, hint, ext } => match client {
                    Some(c) => c.open_stream(&url).map(|(reader, len)| {
                        let c2 = c.clone();
                        let u2 = url.clone();
                        Input {
                            source: InputSource::Stream {
                                reader,
                                len,
                                reopen: Some(Box::new(move |off| c2.open_stream_at(&u2, off))),
                                cache: Some((id, ext)),
                            },
                            hint,
                        }
                    }),
                    None => Err("Not signed in.".into()),
                },
                Plan::Offline => Err("Offline Mode is on and this song isn't downloaded.".into()),
            };
            if let Err(e) = &r {
                crate::log::error(format!("couldn't open a song: {e}"));
            }
            let _ = tx.send(wrap(r));
        });
    }

    pub fn start_current(&mut self, start_at: f64, paused: bool) {
        self.start_current_at(start_at, paused, false);
    }

    fn start_current_at(&mut self, start_at: f64, paused: bool, force_mp3: bool) {
        let Some(song) = self.queue.get(self.qi).cloned() else { return };
        if self.client.is_none() {
            return;
        }
        if self.engine.is_none() {
            let msg = self.engine_error.clone().unwrap_or_else(|| "No audio output.".into());
            self.toast_err(msg);
        }
        let plan = self.plan(&song, self.cfg.settings.streaming_quality, force_mp3);
        if let Plan::Offline = plan {
            // Skip forward to the next song that can play without the network.
            let next = (self.qi + 1..self.queue.len()).find(|&i| {
                let s = &self.queue[i];
                self.downloads.has(s) || store::cached(&s.id).is_some()
            });
            self.toast_err(format!("\"{}\" isn't downloaded, and Offline Mode is on.", song.title));
            match next {
                Some(i) => {
                    self.qi = i;
                    self.start_current_at(0.0, paused, false);
                }
                None => {
                    if let Some(e) = &self.engine {
                        e.send(Cmd::Stop);
                    }
                }
            }
            return;
        }
        self.from_file = matches!(plan, Plan::File(_));
        self.transcoded = force_mp3 || matches!(&plan, Plan::Stream { url, .. } if url.contains("format=mp3"));
        self.gen_seq += 1;
        let g = self.gen_seq;
        self.generation = g;
        self.preload = None;
        let gain = self.gain_for(&song);
        self.open_plan(plan, move |r| Msg::Stream(g, r, start_at, paused, gain));
        self.status = Status { generation: g, loading: true, paused, position: start_at, ..Default::default() };
        self.track_started(&song, paused);
        self.current = Some(song);
        self.dirty = true;
    }

    /// Lyrics, the now-playing scrobble and history, for a song that just began.
    fn track_started(&mut self, song: &Song, paused: bool) {
        self.scrobbled = false;
        if self.lyrics.as_ref().is_none_or(|(id, _)| *id != song.id) {
            self.lyrics = None;
            let s = song.clone();
            let source = self.cfg.settings.lyrics_source;
            self.spawn(move |c| {
                use crate::settings::LyricsSource;
                let l = match source {
                    LyricsSource::Server => c.lyrics(&s),
                    LyricsSource::Lrclib => c.lrclib(&s),
                    LyricsSource::Auto => match c.lyrics(&s) {
                        Some(l @ Lyrics::Synced(_)) => Some(l),
                        other => c.lrclib(&s).or(other),
                    },
                };
                Some(Msg::Lyrics(s.id.clone(), l))
            });
        }
        if !paused && !self.cfg.settings.offline_mode {
            let id = song.id.clone();
            self.spawn(move |c| {
                let _ = c.scrobble(&id, false);
                None
            });
        }
        self.history.push(song.id.clone());
        if self.history.len() > 500 {
            self.history.drain(..100);
        }
    }

    fn next_index(&self) -> Option<usize> {
        if self.queue.is_empty() {
            return None;
        }
        if self.repeat == Repeat::One {
            return Some(self.qi);
        }
        if self.qi + 1 < self.queue.len() {
            Some(self.qi + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            None
        }
    }

    fn tick_player(&mut self) {
        let Some(st) = self.engine.as_ref().map(|e| e.status()) else { return };

        // Gapless or a crossfade carried on into the preloaded song.
        if let Some((pg, pqi, pid)) = self.preload.clone() {
            if st.generation == pg && self.generation != pg {
                self.preload = None;
                self.generation = pg;
                let idx = if self.queue.get(pqi).is_some_and(|s| s.id == pid) {
                    Some(pqi)
                } else {
                    self.queue.iter().position(|s| s.id == pid)
                };
                if let Some(i) = idx {
                    self.qi = i;
                    let song = self.queue[i].clone();
                    self.track_started(&song, false);
                    self.current = Some(song);
                }
                self.dirty = true;
            }
        }
        if st.generation != self.generation {
            return;
        }
        let ended = st.ended;
        if ended {
            if let Some(e) = &self.engine {
                e.status.lock().unwrap().ended = false;
            }
        }
        let changed = (st.position - self.status.position).abs() >= 0.25
            || st.paused != self.status.paused
            || st.loading != self.status.loading
            || st.playing != self.status.playing;
        let keep_loading = self.status.loading && !st.playing && !ended && st.error.is_none();
        self.status = st.clone();
        if keep_loading {
            self.status.loading = true;
        }
        if changed {
            self.dirty = true;
        }

        if !self.scrobbled && !self.cfg.settings.offline_mode {
            if let Some(cur) = &self.current {
                let dur = st.duration.unwrap_or(cur.duration as f64);
                if dur > 30.0 && st.position >= (dur * 0.5).min(240.0) {
                    self.scrobbled = true;
                    let id = cur.id.clone();
                    self.spawn(move |c| {
                        let _ = c.scrobble(&id, true);
                        None
                    });
                }
            }
        }

        // A song that should be playing but has gone quiet for 10 seconds,
        // because the network stalled or a decoder got stuck, starts again
        // from the same spot.
        if st.loading && !st.paused && self.current.is_some() && !ended {
            let since = *self.stall_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(10) {
                self.stall_since = None;
                if self.stall_retries < 3 {
                    self.stall_retries += 1;
                    let at = st.position;
                    crate::log::warn(format!("no audio for 10 s at {at:.1}s, reopening the song (try {})", self.stall_retries));
                    self.start_current_at(at, false, false);
                    return;
                }
                self.toast_err("This song stopped loading. Check the connection, or pick it again.");
            }
        } else if !st.loading {
            self.stall_since = None;
            if st.playing && !st.paused {
                self.stall_retries = 0;
            }
        }

        if ended {
            if let Some(err) = st.error {
                if !self.transcoded && !self.from_file {
                    // Retry once with the server transcoding to MP3.
                    crate::log::info("retrying the song as an MP3 transcode");
                    self.start_current_at(0.0, false, true);
                    return;
                }
                self.toast_err(err);
            }
            self.advance(true);
            return;
        }
        self.maybe_preload(&st);
    }

    /// Opens the next song early so gapless playback and crossfade can join it.
    fn maybe_preload(&mut self, st: &Status) {
        let s = &self.cfg.settings;
        let want = self.next_index();
        if let Some((_, pqi, pid)) = &self.preload {
            if want != Some(*pqi) || self.queue.get(*pqi).map(|x| &x.id) != Some(pid) {
                if let Some(e) = &self.engine {
                    e.send(Cmd::ClearPreload);
                }
                self.preload = None;
            }
        }
        if self.preload.is_some() || !(s.gapless || s.crossfade > 0) || !st.playing || st.loading || st.paused {
            return;
        }
        let Some(cur) = &self.current else { return };
        let dur = st.duration.unwrap_or(cur.duration as f64);
        if dur <= 0.0 || dur - st.position > s.crossfade as f64 + 25.0 {
            return;
        }
        let Some(i) = want else { return };
        let song = self.queue[i].clone();
        let plan = self.plan(&song, s.streaming_quality, false);
        if let Plan::Offline = plan {
            return;
        }
        self.gen_seq += 1;
        let g = self.gen_seq;
        self.preload = Some((g, i, song.id.clone()));
        let gain = self.gain_for(&song);
        self.open_plan(plan, move |r| Msg::Preloaded(g, r, gain));
    }

    /// Fetches the next few songs into the cache once the current one has
    /// finished downloading.
    fn tick_prefetch(&mut self) {
        let ahead = self.cfg.settings.cache_ahead as usize;
        if ahead == 0
            || self.prefetching.is_some()
            || self.cfg.settings.offline_mode
            || self.last_prefetch.elapsed() < Duration::from_secs(2)
            || self.current.is_none()
        {
            return;
        }
        self.last_prefetch = Instant::now();
        let current_done = self.from_file || self.status.buffered + 0.5 >= self.duration();
        if !current_done {
            return;
        }
        let Some(client) = self.client.clone() else { return };
        for k in 1..=ahead {
            let Some(song) = self.queue.get(self.qi + k).cloned() else { break };
            if self.downloads.has(&song) || store::cached(&song.id).is_some() {
                continue;
            }
            if self.preload.as_ref().is_some_and(|p| p.2 == song.id) {
                continue;
            }
            let Plan::Stream { url, id, ext, .. } = self.plan(&song, self.cfg.settings.streaming_quality, false) else {
                continue;
            };
            self.prefetching = Some(id.clone());
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let dest = store::cache_path(&id, &ext);
                match store::fetch_to(&client, &url, &dest, None) {
                    Ok(_) => store::trim_cache(Some(&dest)),
                    Err(e) => crate::log::warn(format!("cache ahead failed: {e}")),
                }
                let _ = tx.send(Msg::Prefetched(id));
            });
            break;
        }
    }

    fn advance(&mut self, auto: bool) {
        if self.queue.is_empty() {
            return;
        }
        if auto && self.repeat == Repeat::One {
            self.start_current(0.0, false);
            return;
        }
        if self.qi + 1 < self.queue.len() {
            self.qi += 1;
            self.start_current(0.0, false);
        } else if self.repeat == Repeat::All {
            self.qi = 0;
            self.start_current(0.0, false);
        } else if auto {
            self.status.paused = true;
            self.status.playing = false;
            self.status.position = 0.0;
            if let Some(e) = &self.engine {
                e.send(Cmd::Stop);
            }
            if self.cfg.settings.autoplay && !self.cfg.settings.offline_mode {
                if let Some(cur) = &self.current {
                    let id = cur.id.clone();
                    self.spawn(move |c| match c.similar_songs(&id, 30) {
                        Ok(s) if !s.is_empty() => Some(Msg::Autoplay(s)),
                        _ => None,
                    });
                }
            }
        } else {
            self.toast("End of the queue");
        }
    }

    pub fn next(&mut self) {
        self.advance(false);
    }

    pub fn prev(&mut self) {
        if self.status.position > 3.0 || self.qi == 0 {
            self.seek_to(0.0);
            return;
        }
        self.qi -= 1;
        self.start_current(0.0, false);
    }

    pub fn toggle(&mut self) {
        if self.current.is_none() {
            if !self.queue.is_empty() {
                self.start_current(0.0, false);
            } else {
                self.toast("Pick something to play: Enter plays the selection.");
            }
            return;
        }
        if !self.status.playing && !self.status.loading {
            let at = self.status.position;
            self.start_current(at, false);
            return;
        }
        if let Some(e) = &self.engine {
            e.send(Cmd::Toggle);
        }
        if self.status.paused && !self.cfg.settings.offline_mode {
            if let Some(cur) = &self.current {
                let id = cur.id.clone();
                self.spawn(move |c| {
                    let _ = c.scrobble(&id, false);
                    None
                });
            }
        }
        self.status.paused = !self.status.paused;
        self.dirty = true;
    }

    pub fn duration(&self) -> f64 {
        self.status
            .duration
            .or(self.current.as_ref().map(|c| c.duration as f64))
            .unwrap_or(0.0)
    }

    pub fn seek_to(&mut self, secs: f64) {
        if self.current.is_none() {
            return;
        }
        if !self.status.seekable && self.status.playing {
            self.toast("This stream can't seek. It will once it's cached.");
            return;
        }
        let d = self.duration();
        let t = if d > 0.0 { secs.clamp(0.0, (d - 0.5).max(0.0)) } else { secs.max(0.0) };
        if let Some(e) = &self.engine {
            e.send(Cmd::Seek(t));
        }
        self.status.position = t;
        self.dirty = true;
    }

    pub fn seek_by(&mut self, d: f64) {
        let p = self.status.position + d;
        self.seek_to(p);
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(e) = &self.engine {
            e.send(Cmd::Volume(self.volume * self.volume));
        }
        self.dirty = true;
    }

    pub fn jump_queue(&mut self, i: usize) {
        if i < self.queue.len() {
            self.qi = i;
            self.start_current(0.0, false);
        }
    }

    // ------------------------------------------------------------ downloads

    fn download_job(&self, s: &Song) -> Option<Job> {
        let q = self.cfg.settings.download_quality;
        match self.plan_stream(s, q) {
            Some((url, ext)) => Some(Job { song: s.clone(), url, ext, quality: q.short().into() }),
            None => None,
        }
    }

    /// A stream URL that ignores the cache, for downloads.
    fn plan_stream(&self, s: &Song, q: Quality) -> Option<(String, String)> {
        let c = self.client.as_ref()?;
        let kbps = q.kbps();
        let cap = kbps > 0 && s.bitrate.is_none_or(|b| b > kbps) && (!self.cfg.settings.transcode_lossless_only || lossless(s));
        let (br, fmt) = if !decodable(s) {
            (if kbps > 0 { kbps } else { 320 }, Some("mp3"))
        } else if cap {
            (kbps, Some("mp3"))
        } else {
            (0, None)
        };
        Some((c.stream_url(&s.id, br, fmt), store::ext_for(s, fmt)))
    }

    pub fn download(&mut self, songs: Vec<Song>, label: &str) {
        if self.cfg.settings.offline_mode {
            self.toast_err("Turn off Offline Mode to download.");
            return;
        }
        let Some(tx) = self.dl_tx.clone() else { return };
        let mut n = 0;
        for s in songs {
            if self.downloads.has(&s) || self.dl_pending.contains(&s.id) {
                continue;
            }
            if let Some(job) = self.download_job(&s) {
                self.dl_pending.insert(s.id.clone());
                let _ = tx.send(job);
                n += 1;
            }
        }
        if n == 0 {
            self.toast("Already downloaded");
            return;
        }
        self.dl_progress.lock().unwrap().queued += n;
        self.toast(if label.is_empty() {
            format!("Downloading {n} song{}", if n == 1 { "" } else { "s" })
        } else {
            format!("Downloading {label}")
        });
        self.refresh_downloads_view();
    }

    pub fn remove_downloads(&mut self, songs: &[Song]) {
        let mut n = 0;
        for s in songs {
            if self.downloads.remove(s) {
                n += 1;
            }
        }
        self.downloaded_ids = self.downloads.items.iter().map(|d| d.id.clone()).collect();
        self.toast(format!("Removed {n} download{}", if n == 1 { "" } else { "s" }));
        self.refresh_downloads_view();
    }

    // ------------------------------------------------------------ search

    fn tick_search(&mut self) {
        let Some(v) = self.stack.last_mut() else { return };
        let uid = v.uid;
        let Kind::Search(s) = &mut v.kind else { return };
        let q = s.query.trim().to_string();
        if q == s.searched || s.last_edit.elapsed() < Duration::from_millis(220) {
            return;
        }
        s.searched = q.clone();
        if q.is_empty() {
            s.ranked = None;
            s.busy = false;
            self.dirty = true;
            return;
        }
        s.busy = true;
        self.dirty = true;
        self.spawn(move |c| {
            Some(match c.search(&q) {
                Ok(r) => Msg::Search(uid, q, r),
                Err(e) => Msg::Error(uid, e),
            })
        });
    }

    fn remember_search(&mut self) {
        let q = match self.top_kind() {
            Some(Kind::Search(s)) => s.searched.clone(),
            _ => return,
        };
        self.cfg.settings.remember_search(&q);
    }

    // ------------------------------------------------------------ selection

    /// The item under the cursor, plus the list of songs it was picked from.
    pub fn selection(&self) -> Option<(Item, Vec<Song>, usize)> {
        let v = self.top()?;
        match &v.kind {
            Kind::Home { shelves, row, .. } => {
                let s = shelves.get(*row)?;
                Some((s.items.get(s.sel)?.clone(), Vec::new(), 0))
            }
            Kind::Search(s) => {
                if s.editing {
                    return None;
                }
                let r = s.ranked.as_ref()?;
                match s.sec {
                    SEC_TOP => {
                        let t = r.top.clone()?;
                        if let Item::Song(song) = &t {
                            let i = r.songs.iter().position(|x| x.id == song.id).unwrap_or(0);
                            return Some((t, r.songs.clone(), i));
                        }
                        Some((t, Vec::new(), 0))
                    }
                    SEC_SONGS => {
                        let song = r.songs.get(s.song_sel)?;
                        Some((Item::Song(song.clone()), r.songs.clone(), s.song_sel))
                    }
                    sec => {
                        let (sel, _) = s.shelf[sec - SEC_ARTISTS];
                        Some((s.shelf_items(sec).get(sel)?.clone(), Vec::new(), 0))
                    }
                }
            }
            Kind::Grid { grid, .. } => Some((grid.selected()?.clone(), Vec::new(), 0)),
            Kind::Tracks { songs, sel, .. } => Some((Item::Song(songs.get(*sel)?.clone()), songs.clone(), *sel)),
            Kind::Artist { top, top_sel, albums, section, .. } => {
                if *section == 0 {
                    Some((Item::Song(top.get(*top_sel)?.clone()), top.clone(), *top_sel))
                } else {
                    Some((albums.selected()?.clone(), Vec::new(), 0))
                }
            }
            Kind::Queue { sel, .. } => Some((Item::Song(self.queue.get(*sel)?.clone()), Vec::new(), *sel)),
            Kind::NowPlaying => Some((Item::Song(self.current.clone()?), Vec::new(), 0)),
            Kind::Settings { .. } => None,
        }
    }

    fn view_context(&self) -> String {
        match self.top() {
            Some(v) if matches!(v.kind, Kind::Search(_)) => "Search".into(),
            Some(v) => v.title.clone(),
            None => String::new(),
        }
    }

    pub fn activate(&mut self) {
        match self.top_kind() {
            Some(Kind::Queue { .. }) => {
                if let Some((_, _, idx)) = self.selection() {
                    self.jump_queue(idx);
                }
                return;
            }
            Some(Kind::Settings { .. }) => {
                self.settings_activate();
                return;
            }
            Some(Kind::Search(s)) if !s.editing && s.sec == SEC_SONGS && s.song_sel >= SEARCH_SONGS => {
                let (q, songs) = (s.searched.clone(), s.ranked.as_ref().map(|r| r.songs.clone()).unwrap_or_default());
                self.remember_search();
                self.open_songs(format!("Songs for \"{q}\""), songs);
                return;
            }
            Some(Kind::Search(_)) => self.remember_search(),
            _ => {}
        }
        let Some((item, list, idx)) = self.selection() else { return };
        let ctx = self.view_context();
        match item {
            Item::Song(s) => {
                if list.is_empty() {
                    self.play_list(vec![s], 0, false, ctx);
                } else {
                    self.play_list(list, idx, false, ctx);
                }
            }
            Item::Album(a) => self.open_album(a.id),
            Item::Artist(a) => self.open_artist(a.id),
            Item::Playlist(p) => self.open_playlist(p.id),
        }
    }

    pub fn fetch_songs(&mut self, coll: Collection, mode: PlayMode, label: String) {
        self.spawn(move |c| {
            let r = match &coll {
                Collection::Album(id) => c.album(id).map(|x| x.1),
                Collection::Playlist(id) => c.playlist(id).map(|x| x.1),
                Collection::ArtistTop(name) => c.top_songs(name, 50),
                Collection::Radio(id) => c.similar_songs(id, 50),
            };
            Some(match r {
                Ok(songs) => Msg::Songs(mode, songs, label),
                Err(e) => Msg::ToastErr(e),
            })
        });
    }

    pub fn run(&mut self, a: Action) {
        self.overlay = None;
        self.dirty = true;
        match a {
            Action::Play(list, i, ctx) => self.play_list(list, i, false, ctx),
            Action::PlayNext(s) => self.play_next(s, ""),
            Action::Queue(s) => self.enqueue(s, ""),
            Action::Fetch(c, m, l) => self.fetch_songs(c, m, l),
            Action::OpenAlbum(id) => self.open_album(id),
            Action::OpenArtist(id) => self.open_artist(id),
            Action::OpenPlaylist(id) => self.open_playlist(id),
            Action::Star(id, k, on) => self.star(id, k, on),
            Action::PickPlaylist(songs) => self.spawn(move |c| {
                Some(match c.playlists() {
                    Ok(p) => Msg::Playlists(p, songs),
                    Err(e) => Msg::ToastErr(e),
                })
            }),
            Action::AddToPlaylist(id, name, songs) => self.spawn(move |c| {
                for s in &songs {
                    if let Err(e) = c.add_to_playlist(&id, &s.id) {
                        return Some(Msg::ToastErr(e));
                    }
                }
                Some(Msg::Toast(format!("Added to {name}")))
            }),
            Action::NewPlaylist(songs) => {
                self.overlay = Some(Overlay::Prompt(Prompt {
                    title: "New playlist".into(),
                    text: String::new(),
                    purpose: PromptFor::NewPlaylist(songs),
                }));
            }
            Action::SaveQueue => {
                if self.queue.is_empty() {
                    self.toast("The queue is empty.");
                    return;
                }
                self.overlay = Some(Overlay::Prompt(Prompt {
                    title: "Save the queue as a playlist".into(),
                    text: if self.context.is_empty() { "Queue".into() } else { self.context.clone() },
                    purpose: PromptFor::SaveQueue,
                }));
            }
            Action::Download(songs) => self.download(songs, ""),
            Action::RemoveDownload(songs) => self.remove_downloads(&songs),
            Action::QueueJump(i) => self.jump_queue(i),
            Action::QueueRemove(i) => self.queue_remove(i),
            Action::QueueMoveNext(i) => {
                if i < self.queue.len() && i != self.qi {
                    let s = self.queue.remove(i);
                    if i < self.qi {
                        self.qi -= 1;
                    }
                    self.queue.insert(self.qi + 1, s);
                    self.toast("Plays next");
                }
            }
            Action::QueueMoveEnd(i) => {
                if i < self.queue.len() && i != self.qi {
                    let s = self.queue.remove(i);
                    if i < self.qi {
                        self.qi -= 1;
                    }
                    self.queue.push(s);
                }
            }
            Action::QueueClearUpcoming => {
                self.queue.truncate(self.qi + 1);
                self.original = None;
                self.toast("Cleared everything after this song");
            }
            Action::QueueShuffleUpcoming => {
                if self.qi + 1 < self.queue.len() {
                    shuffle_vec(&mut self.queue[self.qi + 1..]);
                    self.toast("Shuffled what's up next");
                }
            }
            Action::SignOut => {
                if let Some(e) = &self.engine {
                    e.send(Cmd::Stop);
                }
                crate::log::info("signed out");
                self.cfg.token.clear();
                self.cfg.salt.clear();
                self.cfg.save();
                self.client = None;
                self.art = None;
                self.queue.clear();
                self.current = None;
                self.stack.clear();
                self.screen = Screen::Login(LoginForm {
                    fields: [self.cfg.server.clone(), self.cfg.username.clone(), String::new()],
                    field: 2,
                    busy: false,
                    error: None,
                });
            }
        }
    }

    fn queue_remove(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        if i == self.qi {
            self.toast("That song is playing. Skip it first.");
            return;
        }
        let removed = self.queue.remove(i);
        if let Some(o) = &mut self.original {
            if let Some(p) = o.iter().position(|s| s.id == removed.id) {
                o.remove(p);
            }
        }
        if i < self.qi {
            self.qi -= 1;
        }
        if let Some(View { kind: Kind::Queue { sel, .. }, .. }) = self.stack.last_mut() {
            *sel = (*sel).min(self.queue.len().saturating_sub(1));
        }
    }

    fn submit_prompt(&mut self) {
        let Some(Overlay::Prompt(p)) = self.overlay.take() else { return };
        let name = p.text.trim().to_string();
        if name.is_empty() {
            self.toast_err("Give the playlist a name.");
            self.overlay = Some(Overlay::Prompt(p));
            return;
        }
        let ids: Vec<String> = match &p.purpose {
            PromptFor::SaveQueue => self.queue.iter().map(|s| s.id.clone()).collect(),
            PromptFor::NewPlaylist(songs) => songs.iter().map(|s| s.id.clone()).collect(),
        };
        self.spawn(move |c| {
            Some(match c.create_playlist(&name, &ids) {
                Ok(()) => Msg::Toast(format!("Saved \"{name}\"")),
                Err(e) => Msg::ToastErr(e),
            })
        });
        self.refresh_my_playlists();
    }

    pub fn open_menu(&mut self, anchor: Option<(u16, u16)>) {
        let in_queue = matches!(self.top_kind(), Some(Kind::Queue { .. }));
        let Some((item, list, idx)) = self.selection() else { return };
        let ctx = self.view_context();
        let mut e: Vec<(String, Action)> = Vec::new();
        let title = item.title().to_string();
        match &item {
            Item::Song(s) => {
                if in_queue {
                    if idx != self.qi {
                        e.push(("▶  Play now".into(), Action::QueueJump(idx)));
                        e.push(("↳  Play next".into(), Action::QueueMoveNext(idx)));
                        e.push(("⤓  Move to the end".into(), Action::QueueMoveEnd(idx)));
                        e.push(("✕  Remove from queue".into(), Action::QueueRemove(idx)));
                    }
                    e.push(("⇄  Shuffle what's up next".into(), Action::QueueShuffleUpcoming));
                    e.push(("⌫  Clear everything after this song".into(), Action::QueueClearUpcoming));
                    e.push(("≡  Save the queue as a playlist…".into(), Action::SaveQueue));
                } else {
                    if !list.is_empty() {
                        e.push(("▶  Play".into(), Action::Play(list.clone(), idx, ctx)));
                    } else {
                        e.push(("▶  Play".into(), Action::Play(vec![s.clone()], 0, ctx)));
                    }
                    e.push(("↳  Play next".into(), Action::PlayNext(vec![s.clone()])));
                    e.push(("+  Add to queue".into(), Action::Queue(vec![s.clone()])));
                }
                e.push(("◉  Start radio".into(), Action::Fetch(Collection::Radio(s.id.clone()), PlayMode::Play, format!("{} radio", s.title))));
                e.push((
                    if s.starred { "♥  Remove from Liked Songs".into() } else { "♡  Add to Liked Songs".into() },
                    Action::Star(s.id.clone(), StarKind::Song, !s.starred),
                ));
                e.push(("≡  Add to playlist…".into(), Action::PickPlaylist(vec![s.clone()])));
                if self.downloads.has(s) {
                    e.push(("✕  Remove download".into(), Action::RemoveDownload(vec![s.clone()])));
                } else {
                    e.push(("↓  Download".into(), Action::Download(vec![s.clone()])));
                }
                if let Some(a) = &s.album_id {
                    e.push(("◫  Go to album".into(), Action::OpenAlbum(a.clone())));
                }
                if let Some(a) = &s.artist_id {
                    e.push(("♫  Go to artist".into(), Action::OpenArtist(a.clone())));
                }
            }
            Item::Album(a) => {
                let c = Collection::Album(a.id.clone());
                e.push(("▶  Play".into(), Action::Fetch(c.clone(), PlayMode::Play, a.name.clone())));
                e.push(("⇄  Shuffle".into(), Action::Fetch(c.clone(), PlayMode::Shuffle, a.name.clone())));
                e.push(("↳  Play next".into(), Action::Fetch(c.clone(), PlayMode::Next, a.name.clone())));
                e.push(("+  Add to queue".into(), Action::Fetch(c.clone(), PlayMode::Queue, a.name.clone())));
                e.push(("↓  Download".into(), Action::Fetch(c, PlayMode::Download, a.name.clone())));
                e.push((
                    if a.starred { "♥  Remove from Liked".into() } else { "♡  Like album".into() },
                    Action::Star(a.id.clone(), StarKind::Album, !a.starred),
                ));
                e.push(("◫  Open album".into(), Action::OpenAlbum(a.id.clone())));
                if let Some(ar) = &a.artist_id {
                    e.push(("♫  Go to artist".into(), Action::OpenArtist(ar.clone())));
                }
            }
            Item::Artist(a) => {
                let c = Collection::ArtistTop(a.name.clone());
                e.push(("▶  Play top songs".into(), Action::Fetch(c.clone(), PlayMode::Play, a.name.clone())));
                e.push(("⇄  Shuffle top songs".into(), Action::Fetch(c, PlayMode::Shuffle, a.name.clone())));
                e.push(("◉  Artist radio".into(), Action::Fetch(Collection::Radio(a.id.clone()), PlayMode::Play, format!("{} radio", a.name))));
                e.push((
                    if a.starred { "♥  Unfollow".into() } else { "♡  Follow".into() },
                    Action::Star(a.id.clone(), StarKind::Artist, !a.starred),
                ));
                e.push(("♫  Open artist".into(), Action::OpenArtist(a.id.clone())));
            }
            Item::Playlist(p) => {
                let c = Collection::Playlist(p.id.clone());
                e.push(("▶  Play".into(), Action::Fetch(c.clone(), PlayMode::Play, p.name.clone())));
                e.push(("⇄  Shuffle".into(), Action::Fetch(c.clone(), PlayMode::Shuffle, p.name.clone())));
                e.push(("↳  Play next".into(), Action::Fetch(c.clone(), PlayMode::Next, p.name.clone())));
                e.push(("+  Add to queue".into(), Action::Fetch(c.clone(), PlayMode::Queue, p.name.clone())));
                e.push(("↓  Download".into(), Action::Fetch(c, PlayMode::Download, p.name.clone())));
                e.push(("≡  Open playlist".into(), Action::OpenPlaylist(p.id.clone())));
            }
        }
        self.overlay = Some(Overlay::Menu(Menu { title, entries: e, sel: 0, anchor }));
        self.dirty = true;
    }

    fn quick(&mut self, mode: PlayMode) {
        let Some((item, _, _)) = self.selection() else { return };
        match item {
            Item::Song(s) => match mode {
                PlayMode::Next => self.play_next(vec![s], ""),
                PlayMode::Download => self.download(vec![s], ""),
                _ => self.enqueue(vec![s], ""),
            },
            Item::Album(a) => self.fetch_songs(Collection::Album(a.id), mode, a.name),
            Item::Playlist(p) => self.fetch_songs(Collection::Playlist(p.id), mode, p.name),
            Item::Artist(a) => self.fetch_songs(Collection::ArtistTop(a.name.clone()), mode, a.name),
        }
    }

    fn like_selection(&mut self) {
        let Some((item, _, _)) = self.selection() else { return };
        match item {
            Item::Song(s) => self.star(s.id, StarKind::Song, !s.starred),
            Item::Album(a) => self.star(a.id, StarKind::Album, !a.starred),
            Item::Artist(a) => self.star(a.id, StarKind::Artist, !a.starred),
            Item::Playlist(_) => {}
        }
    }

    pub fn like_current(&mut self) {
        if let Some(c) = self.current.clone() {
            self.star(c.id, StarKind::Song, !c.starred);
        }
    }

    fn radio(&mut self) {
        let Some((item, _, _)) = self.selection() else { return };
        let label = format!("{} radio", item.title());
        match item {
            Item::Song(s) => self.fetch_songs(Collection::Radio(s.id), PlayMode::Play, label),
            Item::Artist(a) => self.fetch_songs(Collection::Radio(a.id), PlayMode::Play, label),
            Item::Album(a) => self.fetch_songs(Collection::Radio(a.id), PlayMode::Play, label),
            Item::Playlist(_) => return,
        }
        self.toast("Starting radio…");
    }

    fn page_songs(&self) -> Option<Vec<Song>> {
        match self.top_kind()? {
            Kind::Tracks { songs, .. } => Some(songs.clone()),
            Kind::Artist { top, .. } if !top.is_empty() => Some(top.clone()),
            _ => None,
        }
    }

    fn header_play(&mut self, shuffle: bool) {
        let Some(v) = self.top() else { return };
        if let Kind::Artist { top, artist: Some(a), .. } = &v.kind {
            if top.is_empty() {
                let name = a.name.clone();
                let mode = if shuffle { PlayMode::Shuffle } else { PlayMode::Play };
                self.fetch_songs(Collection::ArtistTop(name.clone()), mode, name);
                return;
            }
        }
        let ctx = self.view_context();
        if let Some(songs) = self.page_songs() {
            self.play_list(songs, 0, shuffle, ctx);
        }
    }

    fn header_download(&mut self) {
        let Some(songs) = self.page_songs() else { return };
        let ctx = self.view_context();
        if matches!(self.top_kind(), Some(Kind::Tracks { source: Source::Downloads, .. })) {
            return;
        }
        if songs.iter().all(|s| self.downloads.has(s)) {
            self.remove_downloads(&songs);
        } else {
            self.download(songs, &ctx);
        }
    }

    fn header_star(&mut self) {
        let Some(v) = self.top() else { return };
        let target = match &v.kind {
            Kind::Tracks { header: Some(h), .. } => h.star.clone(),
            Kind::Artist { artist: Some(a), .. } => Some((a.id.clone(), StarKind::Artist, a.starred)),
            _ => None,
        };
        if let Some((id, kind, on)) = target {
            if kind != StarKind::Song {
                if let Some(v) = self.stack.last_mut() {
                    if let Kind::Artist { artist: Some(a), .. } = &mut v.kind {
                        a.starred = !on;
                    }
                }
            }
            self.star(id, kind, !on);
        }
    }

    // ------------------------------------------------------------ keys

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        self.dirty = true;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.quit = true;
            return;
        }
        if let Screen::Login(_) = self.screen {
            self.login_key(k);
            return;
        }
        if let Some(ov) = &mut self.overlay {
            match ov {
                Overlay::Help => self.overlay = None,
                Overlay::Menu(m) => match k.code {
                    KeyCode::Up | KeyCode::Char('k') => m.sel = m.sel.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => m.sel = (m.sel + 1).min(m.entries.len().saturating_sub(1)),
                    KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
                        if let Some((_, a)) = m.entries.get(m.sel).cloned() {
                            self.run(a);
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') | KeyCode::Backspace | KeyCode::Left => {
                        self.overlay = None
                    }
                    _ => {}
                },
                Overlay::Prompt(p) => match k.code {
                    KeyCode::Esc => self.overlay = None,
                    KeyCode::Enter => self.submit_prompt(),
                    KeyCode::Backspace => {
                        p.text.pop();
                    }
                    KeyCode::Char('u') if ctrl => p.text.clear(),
                    KeyCode::Char(c) if !ctrl => p.text.push(c),
                    _ => {}
                },
            }
            return;
        }

        if self.search_key(k) {
            return;
        }

        match k.code {
            KeyCode::Char('q') => {
                self.quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                return;
            }
            KeyCode::Char(' ') => return self.toggle(),
            KeyCode::Char('n') => return self.next(),
            KeyCode::Char('p') => return self.prev(),
            KeyCode::Char(']') | KeyCode::Char('.') => return self.seek_by(10.0),
            KeyCode::Char('[') => return self.seek_by(-10.0),
            KeyCode::Char('=') | KeyCode::Char('+') => return self.set_volume(self.volume + 0.05),
            KeyCode::Char('-') | KeyCode::Char('_') => return self.set_volume(self.volume - 0.05),
            KeyCode::Char('s') => return self.toggle_shuffle(),
            KeyCode::Char('r') => return self.cycle_repeat(),
            KeyCode::Char('f') => return self.open_now_playing(),
            KeyCode::Char('L') => return self.like_current(),
            KeyCode::Char(',') => return self.open_nav(NAV_SETTINGS),
            KeyCode::Char('i') => {
                if let Some(a) = &mut self.art {
                    a.cycle_mode();
                    let name = a.protocol_name();
                    self.toast(format!("Cover art: {name}"));
                }
                return;
            }
            KeyCode::Char('/') => {
                if !matches!(self.top_kind(), Some(Kind::Search(_))) {
                    self.open_nav(1);
                } else if let Some(s) = self.search_mut() {
                    s.editing = true;
                }
                return;
            }
            KeyCode::Char(c @ '1'..='9') => {
                self.open_nav(c as usize - '1' as usize);
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // Next or previous sidebar page.
                let root = self.current_root().unwrap_or(0);
                let n = NAV.len();
                let i = if k.code == KeyCode::Tab { (root + 1) % n } else { (root + n - 1) % n };
                self.open_nav(i);
                return;
            }
            KeyCode::Char('O') => {
                self.open_settings(SPage::Account);
                return;
            }
            _ => {}
        }

        let in_queue = matches!(self.top_kind(), Some(Kind::Queue { .. }));
        let in_settings = matches!(self.top_kind(), Some(Kind::Settings { .. }));
        if in_settings {
            match k.code {
                KeyCode::Left | KeyCode::Char('h') => return self.settings_adjust(-1),
                KeyCode::Right | KeyCode::Char('l') => return self.settings_adjust(1),
                _ => {}
            }
        }
        match k.code {
            KeyCode::Esc | KeyCode::Backspace => return self.back(),
            KeyCode::Enter => return self.activate(),
            KeyCode::Char('a') => return self.quick(PlayMode::Queue),
            KeyCode::Char('A') => return self.quick(PlayMode::Next),
            KeyCode::Char('D') => return self.quick(PlayMode::Download),
            KeyCode::Char('m') | KeyCode::Menu => return self.open_menu(None),
            KeyCode::Char('l') => return self.like_selection(),
            KeyCode::Char('R') => return self.radio(),
            KeyCode::Char('P') => return self.header_play(false),
            KeyCode::Char('S') if in_queue => return self.run(Action::SaveQueue),
            KeyCode::Char('S') => return self.header_play(true),
            KeyCode::Char('c') if in_queue => return self.run(Action::QueueClearUpcoming),
            KeyCode::Char('z') if in_queue => return self.run(Action::QueueShuffleUpcoming),
            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete if in_queue => {
                if let Some(View { kind: Kind::Queue { sel, .. }, .. }) = self.stack.last() {
                    let i = *sel;
                    self.queue_remove(i);
                }
                return;
            }
            KeyCode::Char('J') if in_queue => return self.move_queue_item(1),
            KeyCode::Char('K') if in_queue => return self.move_queue_item(-1),
            KeyCode::Down if in_queue && k.modifiers.contains(KeyModifiers::SHIFT) => return self.move_queue_item(1),
            KeyCode::Up if in_queue && k.modifiers.contains(KeyModifiers::SHIFT) => return self.move_queue_item(-1),
            KeyCode::Char('o') => return self.cycle_sort(),
            _ => {}
        }
        let (dx, dy) = match k.code {
            KeyCode::Up | KeyCode::Char('k') => (0, -1),
            KeyCode::Down | KeyCode::Char('j') => (0, 1),
            KeyCode::Left | KeyCode::Char('h') => (-1, 0),
            KeyCode::Right => (1, 0),
            KeyCode::PageUp => (0, -10),
            KeyCode::PageDown => (0, 10),
            KeyCode::Home | KeyCode::Char('g') => (0, -100000),
            KeyCode::End | KeyCode::Char('G') => (0, 100000),
            _ => return,
        };
        self.move_cursor(dx, dy);
    }

    /// Keys on the Search page. True when the key was used.
    fn search_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let recents = self.cfg.settings.recent_searches.clone();
        let Some(s) = self.search_mut() else { return false };
        let empty = s.query.trim().is_empty();
        if s.editing {
            match k.code {
                KeyCode::Esc => {
                    if !s.query.is_empty() {
                        s.query.clear();
                        s.last_edit = Instant::now();
                    } else {
                        s.editing = false;
                    }
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                    if empty {
                        if !recents.is_empty() {
                            s.editing = false;
                            s.recent_sel = 0;
                        }
                    } else if let Some(first) = s.first() {
                        s.editing = false;
                        s.sec = first;
                        s.song_sel = 0;
                    }
                }
                KeyCode::Backspace => {
                    if ctrl || alt {
                        let t = s.query.trim_end().to_string();
                        let cut = t.rfind(' ').map(|i| i + 1).unwrap_or(0);
                        s.query.truncate(cut);
                    } else {
                        s.query.pop();
                    }
                    s.last_edit = Instant::now();
                }
                KeyCode::Char('w') if ctrl => {
                    let t = s.query.trim_end().to_string();
                    let cut = t.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    s.query.truncate(cut);
                    s.last_edit = Instant::now();
                }
                KeyCode::Char('u') if ctrl => {
                    s.query.clear();
                    s.last_edit = Instant::now();
                }
                KeyCode::Char(c) if !ctrl => {
                    s.query.push(c);
                    s.last_edit = Instant::now();
                }
                _ => return false,
            }
            return true;
        }
        // In the results or the recent searches.
        match k.code {
            KeyCode::Esc => {
                s.editing = true;
                true
            }
            KeyCode::Backspace => {
                s.editing = true;
                s.query.pop();
                s.last_edit = Instant::now();
                true
            }
            KeyCode::Enter if empty => {
                if let Some(q) = recents.get(s.recent_sel) {
                    s.query = q.clone();
                    s.editing = true;
                    s.last_edit = Instant::now() - Duration::from_secs(1);
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') if empty => {
                if s.recent_sel == 0 {
                    s.editing = true;
                } else {
                    s.recent_sel -= 1;
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') if empty => {
                s.recent_sel = (s.recent_sel + 1).min(recents.len().saturating_sub(1));
                true
            }
            KeyCode::Char('x') | KeyCode::Delete if empty => {
                let i = s.recent_sel;
                if i < self.cfg.settings.recent_searches.len() {
                    self.cfg.settings.recent_searches.remove(i);
                    if let Some(s) = self.search_mut() {
                        s.recent_sel = s.recent_sel.saturating_sub(1);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn current_root(&self) -> Option<usize> {
        self.stack.first().map(|v| match &v.kind {
            Kind::Home { .. } => 0,
            Kind::Search(_) => 1,
            Kind::Grid { kind: GridKind::Albums(_), .. } => 2,
            Kind::Grid { kind: GridKind::Artists, .. } => 3,
            Kind::Grid { kind: GridKind::Playlists, .. } => 4,
            Kind::Tracks { source: Source::Downloads, .. } => NAV_DOWNLOADS,
            Kind::Tracks { .. } => 5,
            Kind::Settings { .. } => NAV_SETTINGS,
            _ => NAV_QUEUE,
        })
    }

    pub fn root_index(&self) -> Option<usize> {
        self.current_root()
    }

    fn cycle_sort(&mut self) {
        let Some(v) = self.stack.last_mut() else { return };
        if let Kind::Grid { kind: GridKind::Albums(sort), .. } = &v.kind {
            let i = AlbumSort::ALL.iter().position(|s| s == sort).unwrap_or(0);
            let next = AlbumSort::ALL[(i + 1) % AlbumSort::ALL.len()];
            self.set_sort(next);
        }
    }

    pub fn set_sort(&mut self, sort: AlbumSort) {
        let Some(v) = self.stack.last_mut() else { return };
        let uid = v.uid;
        if let Kind::Grid { kind, grid, exhausted, fetching, .. } = &mut v.kind {
            *kind = GridKind::Albums(sort);
            *grid = Grid::new(Vec::new());
            *exhausted = false;
            *fetching = true;
            v.loading = true;
            v.error = None;
            self.load(uid);
        }
    }

    fn move_queue_item(&mut self, d: i32) {
        let Some(View { kind: Kind::Queue { sel, .. }, .. }) = self.stack.last_mut() else { return };
        let i = *sel;
        let j = i as i32 + d;
        if j < 0 || j as usize >= self.queue.len() {
            return;
        }
        let j = j as usize;
        self.queue.swap(i, j);
        *sel = j;
        if self.qi == i {
            self.qi = j;
        } else if self.qi == j {
            self.qi = i;
        }
    }

    /// Moves queue row `from` to `to`, keeping the playing song in place.
    fn move_queue_to(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() || from == to {
            return;
        }
        let playing = self.queue.get(self.qi).map(|s| s.id.clone());
        let s = self.queue.remove(from);
        self.queue.insert(to, s);
        if from == self.qi {
            self.qi = to;
        } else if let Some(p) = playing {
            let lo = from.min(to);
            let hi = from.max(to);
            if (lo..=hi).contains(&self.qi) {
                if let Some(i) = self.queue.iter().enumerate().position(|(i, x)| x.id == p && (lo..=hi).contains(&i)) {
                    self.qi = i;
                }
            }
        }
        if let Some(View { kind: Kind::Queue { sel, .. }, .. }) = self.stack.last_mut() {
            *sel = to;
        }
    }

    /// Moves the cursor in the current view. False when it hit an edge.
    pub fn move_cursor(&mut self, dx: i32, dy: i32) -> bool {
        let qlen = self.queue.len();
        let settings_rows = match self.top_kind() {
            Some(Kind::Settings { page, .. }) => self.settings_selectable(*page),
            _ => Vec::new(),
        };
        let mut need_more: Option<u64> = None;
        let Some(v) = self.stack.last_mut() else { return false };
        let uid = v.uid;
        let moved = match &mut v.kind {
            Kind::Home { shelves, row, .. } => {
                if shelves.is_empty() {
                    return false;
                }
                if dy != 0 {
                    let n = (*row as i32 + dy.signum() * dy.abs().min(1000)).clamp(0, shelves.len() as i32 - 1);
                    let ok = n as usize != *row;
                    *row = n as usize;
                    ok
                } else {
                    let s = &mut shelves[*row];
                    let n = s.sel as i32 + dx;
                    if n < 0 || n >= s.items.len() as i32 {
                        false
                    } else {
                        s.sel = n as usize;
                        true
                    }
                }
            }
            Kind::Search(s) => search_move(s, dx, dy),
            Kind::Grid { grid, exhausted, fetching, .. } => {
                let ok = if dy.abs() > 1 {
                    grid.page(dy.signum());
                    true
                } else {
                    grid.step(dx, dy)
                };
                if !*exhausted && !*fetching && grid.sel + grid.cols * 3 >= grid.items.len() {
                    *fetching = true;
                    need_more = Some(uid);
                }
                ok
            }
            Kind::Tracks { songs, sel, .. } => {
                if dx != 0 {
                    return false;
                }
                let n = (*sel as i32 + dy).clamp(0, songs.len().saturating_sub(1) as i32) as usize;
                let ok = n != *sel;
                *sel = n;
                ok
            }
            Kind::Queue { sel, .. } => {
                if dx != 0 {
                    return false;
                }
                *sel = (*sel as i32 + dy).clamp(0, qlen.saturating_sub(1) as i32) as usize;
                true
            }
            Kind::Artist { top, top_sel, albums, section, .. } => {
                if *section == 0 {
                    if dx != 0 {
                        return false;
                    }
                    let n = *top_sel as i32 + dy;
                    if n >= top.len() as i32 && !albums.items.is_empty() {
                        *section = 1;
                        albums.sel = 0;
                    } else {
                        *top_sel = n.clamp(0, top.len().saturating_sub(1) as i32) as usize;
                    }
                    true
                } else {
                    let ok = albums.step(dx, dy.signum());
                    if !ok && dy < 0 && !top.is_empty() {
                        *section = 0;
                        return true;
                    }
                    ok
                }
            }
            Kind::Settings { sel, .. } => {
                if dy == 0 || settings_rows.is_empty() {
                    return false;
                }
                let pos = settings_rows.iter().position(|r| *r == *sel).unwrap_or(0) as i32;
                let n = (pos + dy).clamp(0, settings_rows.len() as i32 - 1) as usize;
                *sel = settings_rows[n];
                true
            }
            Kind::NowPlaying => false,
        };
        if let Some(uid) = need_more {
            self.load(uid);
        }
        moved
    }

    // ------------------------------------------------------------ login

    fn login_key(&mut self, k: KeyEvent) {
        let Screen::Login(form) = &mut self.screen else { return };
        if form.busy {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let enter = k.code == KeyCode::Enter || (ctrl && k.code == KeyCode::Char('j'));
        match k.code {
            _ if enter => {
                if form.field < 2 {
                    form.field += 1;
                } else {
                    self.submit_login();
                }
            }
            KeyCode::Char('u') if ctrl => {
                if form.field < 3 {
                    form.fields[form.field].clear();
                }
            }
            KeyCode::Char(_) if ctrl => {}
            KeyCode::Esc => self.quit = true,
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 3) % 4,
            KeyCode::Backspace => {
                if form.field < 3 {
                    form.fields[form.field].pop();
                }
            }
            KeyCode::Char(c) => {
                if form.field < 3 {
                    form.fields[form.field].push(c);
                    form.error = None;
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, s: String) {
        self.dirty = true;
        let s = s.trim_end_matches(['\n', '\r']).to_string();
        if let Some(Overlay::Prompt(p)) = &mut self.overlay {
            p.text.push_str(&s);
            return;
        }
        match &mut self.screen {
            Screen::Login(form) => {
                if form.field < 3 {
                    form.fields[form.field].push_str(&s);
                }
            }
            Screen::Main => {
                if let Some(st) = self.search_mut() {
                    st.query.push_str(&s);
                    st.editing = true;
                    st.last_edit = Instant::now();
                }
            }
        }
    }

    pub fn submit_login(&mut self) {
        let Screen::Login(form) = &mut self.screen else { return };
        let server = api::normalize_server(&form.fields[0]);
        let user = form.fields[1].trim().to_string();
        let pass = form.fields[2].clone();
        if form.fields[0].trim().is_empty() || user.is_empty() || pass.is_empty() {
            form.error = Some("Fill in the server, username and password.".into());
            return;
        }
        form.busy = true;
        form.error = None;
        let tx = self.tx.clone();
        let mut cfg = self.cfg.clone();
        std::thread::spawn(move || {
            let salt = api::random_salt();
            let token = api::token_for(&pass, &salt);
            let client = Client::new(&server, &user, &salt, &token);
            let r = client.ping().map(|_| {
                cfg.server = server.clone();
                cfg.username = user.clone();
                cfg.salt = salt;
                cfg.token = token;
                (client, cfg)
            });
            let _ = tx.send(Msg::Login(r));
        });
    }

    // ------------------------------------------------------------ mouse

    fn hit_at(&self, x: u16, y: u16) -> Option<(Rect, Hit)> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(r, h)| (*r, *h))
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if self.overlay.is_some() {
                    return;
                }
                let d = if matches!(m.kind, MouseEventKind::ScrollDown) { 1 } else { -1 };
                // Over a shelf or with Shift held, the wheel moves sideways.
                let sideways = m.modifiers.contains(KeyModifiers::SHIFT)
                    || matches!(self.hit_at(x, y), Some((_, Hit::SearchShelf(..))));
                if let Some(View { kind: Kind::Home { shelves, row, .. }, .. }) = self.stack.last_mut() {
                    if sideways {
                        if let Some(s) = shelves.get_mut(*row) {
                            s.sel = (s.sel as i32 + d).clamp(0, s.items.len() as i32 - 1) as usize;
                        }
                        self.dirty = true;
                        return;
                    }
                }
                if sideways {
                    self.move_cursor(d, 0);
                } else {
                    self.move_cursor(0, d);
                }
                self.dirty = true;
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                let d = if matches!(m.kind, MouseEventKind::ScrollRight) { 1 } else { -1 };
                self.move_cursor(d, 0);
                self.dirty = true;
            }
            MouseEventKind::Down(btn) => {
                let hit = self.hit_at(x, y);
                let double = self
                    .last_click
                    .is_some_and(|(t, lx, ly)| t.elapsed() < Duration::from_millis(400) && lx == x && ly == y);
                self.last_click = Some((Instant::now(), x, y));
                self.dirty = true;
                if let Some((rect, h)) = hit {
                    self.click(h, rect, x, btn == MouseButton::Right, double);
                } else if self.overlay.is_some() {
                    self.overlay = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                match self.drag {
                    Drag::Seek => {
                        let r = self.seek_rect;
                        if r.width > 0 {
                            let frac = (x.clamp(r.x, r.right() - 1) - r.x) as f64 / r.width as f64;
                            self.seek_preview = Some(frac * self.duration());
                            self.dirty = true;
                        }
                    }
                    Drag::Volume => {
                        let v = self.volume_rect;
                        if v.width > 0 {
                            let frac = (x.clamp(v.x, v.right()) - v.x) as f32 / v.width as f32;
                            self.set_volume(frac);
                        }
                    }
                    Drag::Queue(from) => {
                        if let Some((_, Hit::QueueRow(to))) = self.hit_at(x, y) {
                            if to != from {
                                self.move_queue_to(from, to);
                                self.drag = Drag::Queue(to);
                                self.dirty = true;
                            }
                        }
                    }
                    Drag::Slider(row) => {
                        if let Some((_, r)) = self.slider_rects.iter().find(|(i, _)| *i == row).copied() {
                            let frac = (x.clamp(r.x, r.right().saturating_sub(1)) - r.x) as f32 / r.width.max(2).saturating_sub(1) as f32;
                            self.settings_set_frac(row, frac);
                        }
                    }
                    Drag::None => {}
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.drag == Drag::Seek {
                    if let Some(p) = self.seek_preview.take() {
                        self.seek_to(p);
                    }
                }
                if matches!(self.drag, Drag::Slider(_)) {
                    self.cfg.save();
                }
                self.drag = Drag::None;
            }
            _ => {}
        }
    }

    fn click(&mut self, h: Hit, rect: Rect, x: u16, right: bool, double: bool) {
        if !matches!(h, Hit::MenuItem(_) | Hit::Outside) && self.overlay.is_some() {
            self.overlay = None;
            return;
        }
        match h {
            Hit::Outside => self.overlay = None,
            Hit::MenuItem(i) => {
                if let Some(Overlay::Menu(m)) = &self.overlay {
                    if let Some((_, a)) = m.entries.get(i).cloned() {
                        self.run(a);
                    }
                }
            }
            Hit::Nav(i) => self.open_nav(i),
            Hit::LoginField(i) => {
                if let Screen::Login(f) = &mut self.screen {
                    f.field = i;
                }
            }
            Hit::LoginSubmit => self.submit_login(),
            Hit::Prev => self.prev(),
            Hit::Toggle => self.toggle(),
            Hit::Next => self.next(),
            Hit::Shuffle => self.toggle_shuffle(),
            Hit::Repeat => self.cycle_repeat(),
            Hit::LikeCurrent => self.like_current(),
            Hit::OpenNowPlaying => self.open_now_playing(),
            Hit::OpenQueue => self.open_nav(NAV_QUEUE),
            Hit::Back => self.back(),
            Hit::Crumb(d) => self.back_to(d),
            Hit::Seek => {
                if self.current.is_some() {
                    let frac = (x.saturating_sub(rect.x)) as f64 / rect.width.max(1) as f64;
                    self.seek_preview = Some(frac * self.duration());
                    self.drag = Drag::Seek;
                }
            }
            Hit::Volume => {
                let frac = (x.saturating_sub(rect.x) + 1) as f32 / rect.width.max(1) as f32;
                self.set_volume(frac);
                self.drag = Drag::Volume;
            }
            Hit::HeaderPlay => self.header_play(false),
            Hit::HeaderShuffle => self.header_play(true),
            Hit::HeaderStar => self.header_star(),
            Hit::HeaderDownload => self.header_download(),
            Hit::QueueShuffle => self.run(Action::QueueShuffleUpcoming),
            Hit::QueueClear => self.run(Action::QueueClearUpcoming),
            Hit::QueueSave => self.run(Action::SaveQueue),
            Hit::SortChip(i) => self.set_sort(AlbumSort::ALL[i]),
            Hit::SearchBox => {
                if let Some(s) = self.search_mut() {
                    s.editing = true;
                }
            }
            Hit::Recent(i) => {
                let q = self.cfg.settings.recent_searches.get(i).cloned();
                if let (Some(q), Some(s)) = (q, self.search_mut()) {
                    s.query = q;
                    s.editing = true;
                    s.last_edit = Instant::now() - Duration::from_secs(1);
                }
            }
            Hit::SettingRow(i) => {
                if let Some(View { kind: Kind::Settings { sel, .. }, .. }) = self.stack.last_mut() {
                    *sel = i;
                }
                if right {
                    self.settings_adjust(-1);
                } else {
                    self.settings_activate();
                }
            }
            Hit::SettingSlider(i) => {
                if let Some(View { kind: Kind::Settings { sel, .. }, .. }) = self.stack.last_mut() {
                    *sel = i;
                }
                let frac = (x.saturating_sub(rect.x)) as f32 / rect.width.max(2).saturating_sub(1) as f32;
                self.settings_set_frac(i, frac);
                self.drag = Drag::Slider(i);
            }
            Hit::SearchTop | Hit::SearchSong(_) | Hit::SearchSeeAll | Hit::SearchShelf(..) => {
                if let Some(s) = self.search_mut() {
                    s.editing = false;
                    match h {
                        Hit::SearchTop => s.sec = SEC_TOP,
                        Hit::SearchSong(i) => {
                            s.sec = SEC_SONGS;
                            s.song_sel = i;
                        }
                        Hit::SearchSeeAll => {
                            s.sec = SEC_SONGS;
                            s.song_sel = SEARCH_SONGS;
                        }
                        Hit::SearchShelf(sec, i) => {
                            s.sec = sec;
                            s.shelf[sec - SEC_ARTISTS].0 = i;
                        }
                        _ => {}
                    }
                }
                let card = matches!(h, Hit::SearchTop | Hit::SearchShelf(..) | Hit::SearchSeeAll);
                if right {
                    self.open_menu(Some((x, rect.y)));
                } else if card || double {
                    self.activate();
                }
            }
            Hit::QueueRow(i) => {
                if let Some(View { kind: Kind::Queue { sel, .. }, .. }) = self.stack.last_mut() {
                    *sel = i;
                }
                if right {
                    self.open_menu(Some((x, rect.y)));
                } else if double {
                    self.activate();
                } else {
                    self.drag = Drag::Queue(i);
                }
            }
            Hit::Shelf(..) | Hit::GridItem(_) | Hit::Track(_) | Hit::TopSong(_) | Hit::ArtistAlbum(_) => {
                let opens = self.select_hit(h);
                if right {
                    self.open_menu(Some((x, rect.y)));
                } else if opens || double {
                    self.activate();
                }
            }
        }
    }

    /// Moves the selection to a clicked item. True for cards, which open on one click.
    fn select_hit(&mut self, h: Hit) -> bool {
        let Some(v) = self.stack.last_mut() else { return false };
        match (&mut v.kind, h) {
            (Kind::Home { shelves, row, .. }, Hit::Shelf(r, i)) => {
                *row = r;
                if let Some(s) = shelves.get_mut(r) {
                    s.sel = i;
                }
                true
            }
            (Kind::Grid { grid, .. }, Hit::GridItem(i)) => {
                grid.sel = i;
                true
            }
            (Kind::Tracks { sel, .. }, Hit::Track(i)) => {
                *sel = i;
                false
            }
            (Kind::Artist { top_sel, section, .. }, Hit::TopSong(i)) => {
                *section = 0;
                *top_sel = i;
                false
            }
            (Kind::Artist { albums, section, .. }, Hit::ArtistAlbum(i)) => {
                *section = 1;
                albums.sel = i;
                true
            }
            _ => false,
        }
    }

    /// Redraws this often while something moves on screen.
    pub fn frame_interval(&self) -> Option<Duration> {
        if self.viz_visible() && !self.status.paused {
            return Some(Duration::from_millis(50));
        }
        let busy = self.status.loading
            || (self.current.is_some() && !self.status.paused)
            || matches!(&self.screen, Screen::Login(f) if f.busy)
            || self.dl_progress.lock().map(|p| p.current.is_some()).unwrap_or(false);
        busy.then(|| Duration::from_millis(200))
    }
}

fn search_move(s: &mut SearchState, dx: i32, dy: i32) -> bool {
    if s.ranked.is_none() {
        return false;
    }
    let dy = dy.signum();
    match s.sec {
        SEC_TOP => {
            if dx > 0 && s.has(SEC_SONGS) {
                s.sec = SEC_SONGS;
                s.song_sel = 0;
            } else if dy < 0 {
                s.editing = true;
            } else if dy > 0 {
                if let Some(n) = s.after(SEC_TOP) {
                    s.sec = n;
                } else if s.has(SEC_SONGS) {
                    s.sec = SEC_SONGS;
                }
            }
            true
        }
        SEC_SONGS => {
            if dx < 0 && s.has(SEC_TOP) {
                s.sec = SEC_TOP;
                return true;
            }
            if dy < 0 {
                if s.song_sel == 0 {
                    s.editing = true;
                } else {
                    s.song_sel -= 1;
                }
            } else if dy > 0 {
                if s.song_sel + 1 < s.song_rows() {
                    s.song_sel += 1;
                } else if let Some(n) = s.after(SEC_SONGS) {
                    s.sec = n;
                }
            }
            true
        }
        sec => {
            let i = sec - SEC_ARTISTS;
            let len = s.shelf_len(sec);
            if dx != 0 {
                let n = (s.shelf[i].0 as i32 + dx).clamp(0, len.saturating_sub(1) as i32) as usize;
                s.shelf[i].0 = n;
                return true;
            }
            if dy < 0 {
                match s.before(sec) {
                    Some(p) => s.sec = p,
                    None => s.editing = true,
                }
            } else if dy > 0 {
                if let Some(n) = s.after(sec) {
                    s.sec = n;
                }
            }
            true
        }
    }
}

fn greeting() -> String {
    let hour = std::process::Command::new("date")
        .arg("+%H")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(12);
    match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
    .into()
}
