//! Preferences, named and grouped like Settings in the iPhone app. They live
//! in the config file under
//! `[settings]`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ShuffleMode {
    FewerRepeats,
    Standard,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayGain {
    Off,
    Track,
    Album,
}

impl ReplayGain {
    pub fn label(self) -> &'static str {
        match self {
            ReplayGain::Off => "Off",
            ReplayGain::Track => "Track",
            ReplayGain::Album => "Album",
        }
    }
}

/// `QualityTier`: the kbps cap, or 0 for the file as stored.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Low,
    Medium,
    High,
    Original,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Low, Quality::Medium, Quality::High, Quality::Original];
    pub fn kbps(self) -> u32 {
        match self {
            Quality::Low => 128,
            Quality::Medium => 256,
            Quality::High => 320,
            Quality::Original => 0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low · 128 kbps",
            Quality::Medium => "Medium · 256 kbps",
            Quality::High => "High · 320 kbps",
            Quality::Original => "Original",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
            Quality::Original => "Original",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LyricsSource {
    Server,
    Lrclib,
    Auto,
}

impl LyricsSource {
    pub const ALL: [LyricsSource; 3] = [LyricsSource::Auto, LyricsSource::Server, LyricsSource::Lrclib];
    pub fn label(self) -> &'static str {
        match self {
            LyricsSource::Server => "Server",
            LyricsSource::Lrclib => "LRCLIB (public)",
            LyricsSource::Auto => "Auto (server first)",
        }
    }
}

/// The accent colors the iPhone app offers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Green,
    Blue,
    Violet,
    Rose,
    Orange,
    White,
}

impl Accent {
    pub const ALL: [Accent; 6] = [Accent::Green, Accent::Blue, Accent::Violet, Accent::Rose, Accent::Orange, Accent::White];
    pub fn rgb(self) -> (u8, u8, u8) {
        let hex: u32 = match self {
            Accent::Green => 0x2BE06B,
            Accent::Blue => 0x76B5FF,
            Accent::Violet => 0xBC9AFF,
            Accent::Rose => 0xFF92B8,
            Accent::Orange => 0xFFB36B,
            Accent::White => 0xEEEEEE,
        };
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }
    pub fn label(self) -> &'static str {
        match self {
            Accent::Green => "Green",
            Accent::Blue => "Blue",
            Accent::Violet => "Violet",
            Accent::Rose => "Rose",
            Accent::Orange => "Orange",
            Accent::White => "White",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Visualizer {
    Bars,
    Mirror,
    Ribbon,
    Off,
}

impl Visualizer {
    pub const ALL: [Visualizer; 4] = [Visualizer::Bars, Visualizer::Mirror, Visualizer::Ribbon, Visualizer::Off];
    pub fn label(self) -> &'static str {
        match self {
            Visualizer::Bars => "Bars",
            Visualizer::Mirror => "Mirror",
            Visualizer::Ribbon => "Ribbon",
            Visualizer::Off => "Off",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StartPage {
    Home,
    Search,
    Albums,
    Playlists,
    Liked,
    Downloads,
}

impl StartPage {
    pub const ALL: [StartPage; 6] =
        [StartPage::Home, StartPage::Search, StartPage::Albums, StartPage::Playlists, StartPage::Liked, StartPage::Downloads];
    pub fn label(self) -> &'static str {
        match self {
            StartPage::Home => "Home",
            StartPage::Search => "Search",
            StartPage::Albums => "Albums",
            StartPage::Playlists => "Playlists",
            StartPage::Liked => "Liked Songs",
            StartPage::Downloads => "Downloads",
        }
    }
}

pub const EQ_BANDS: [f64; 10] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const EQ_MAX_DB: f32 = 12.0;

/// `EqualizerPreset`, same gains.
pub const EQ_PRESETS: [(&str, [f32; 10]); 12] = [
    ("Flat", [0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]),
    ("Bass Boost", [6., 5., 4., 2., 0., 0., 0., 0., 0., 0.]),
    ("Bass Reducer", [-6., -5., -4., -2., 0., 0., 0., 0., 0., 0.]),
    ("Treble Boost", [0., 0., 0., 0., 0., 0., 2., 4., 5., 6.]),
    ("Treble Reducer", [0., 0., 0., 0., 0., 0., -2., -4., -5., -6.]),
    ("Vocal", [-2., -2., -1., 1., 3., 4., 3., 2., 0., -1.]),
    ("Acoustic", [3., 3., 2., 0., 1., 1., 2., 3., 3., 2.]),
    ("Electronic", [4., 4., 2., 0., -1., 1., 0., 1., 3., 4.]),
    ("Hip-Hop", [5., 4., 2., 3., -1., -1., 1., 0., 1., 2.]),
    ("Rock", [4., 3., 2., 0., -1., 0., 2., 3., 4., 4.]),
    ("Jazz", [3., 2., 1., 2., -1., -1., 0., 1., 2., 3.]),
    ("Small Speakers", [-6., -4., 0., 3., 4., 3., 2., 1., -1., -3.]),
];

pub fn preset_name(gains: &[f32; 10]) -> &'static str {
    EQ_PRESETS
        .iter()
        .find(|(_, g)| g.iter().zip(gains).all(|(a, b)| (a - b).abs() < 0.01))
        .map(|(n, _)| *n)
        .unwrap_or("Custom")
}

pub const CACHE_LIMITS_MB: [u64; 6] = [256, 512, 1024, 2048, 5120, 10240];

pub fn mb_label(mb: u64) -> String {
    if mb >= 1024 { format!("{} GB", mb / 1024) } else { format!("{mb} MB") }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    // Playback
    pub shuffle_mode: ShuffleMode,
    pub crossfade: u32,
    pub gapless: bool,
    pub autoplay: bool,
    pub replay_gain: ReplayGain,
    pub replay_gain_preamp: f32,
    pub prevent_clipping: bool,
    pub restore_queue: bool,
    // Equalizer
    pub eq_enabled: bool,
    pub eq_preamp: f32,
    pub eq_gains: [f32; 10],
    // Lyrics
    pub lyrics_source: LyricsSource,
    pub lyrics_auto_scroll: bool,
    // Audio quality
    pub streaming_quality: Quality,
    pub transcode_lossless_only: bool,
    // Downloads and offline
    pub offline_mode: bool,
    pub download_quality: Quality,
    pub cache_ahead: u32,
    pub cache_limit_mb: u64,
    // Look and feel
    pub accent: Accent,
    pub visualizer: Visualizer,
    pub bar_visualizer: bool,
    pub show_audio_quality: bool,
    pub start_page: StartPage,
    // Search and content
    pub hide_explicit: bool,
    pub recent_searches: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shuffle_mode: ShuffleMode::FewerRepeats,
            crossfade: 0,
            gapless: true,
            autoplay: true,
            replay_gain: ReplayGain::Off,
            replay_gain_preamp: 0.0,
            prevent_clipping: true,
            restore_queue: true,
            eq_enabled: false,
            eq_preamp: 0.0,
            eq_gains: [0.0; 10],
            lyrics_source: LyricsSource::Auto,
            lyrics_auto_scroll: true,
            streaming_quality: Quality::Original,
            transcode_lossless_only: false,
            offline_mode: false,
            download_quality: Quality::Original,
            cache_ahead: 2,
            cache_limit_mb: 2048,
            accent: Accent::Green,
            visualizer: Visualizer::Bars,
            bar_visualizer: true,
            show_audio_quality: true,
            start_page: StartPage::Home,
            hide_explicit: false,
            recent_searches: Vec::new(),
        }
    }
}

impl Settings {
    pub fn remember_search(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() {
            return;
        }
        self.recent_searches.retain(|x| !x.eq_ignore_ascii_case(q));
        self.recent_searches.insert(0, q.to_string());
        self.recent_searches.truncate(10);
    }
}
