//! Settings pages. The root list and its groups follow the iPhone app:
//! the account, then Listening, Downloads and offline, Look and feel, Library,
//! and About. Each page has one subject.

use super::{App, Kind, View, fmt_bytes};
use crate::settings::*;
use crate::store;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SPage {
    Root,
    Account,
    Playback,
    Equalizer,
    Lyrics,
    Quality,
    Downloads,
    Appearance,
    Player,
    HomeLibrary,
    SearchContent,
    About,
}

impl SPage {
    pub fn title(self) -> &'static str {
        match self {
            SPage::Root => "Settings",
            SPage::Account => "Account and server",
            SPage::Playback => "Playback",
            SPage::Equalizer => "Equalizer",
            SPage::Lyrics => "Lyrics",
            SPage::Quality => "Audio quality",
            SPage::Downloads => "Downloads and storage",
            SPage::Appearance => "Appearance",
            SPage::Player => "Player",
            SPage::HomeLibrary => "Home and Library",
            SPage::SearchContent => "Search and content",
            SPage::About => "About and help",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SKey {
    OfflineMode,
    Shuffle,
    Crossfade,
    Gapless,
    Autoplay,
    ReplayGain,
    RgPreamp,
    PreventClipping,
    RestoreQueue,
    EqEnabled,
    EqPreset,
    EqPreamp,
    EqBand(usize),
    LyricsSource,
    LyricsScroll,
    StreamQuality,
    LosslessOnly,
    DownloadQuality,
    CacheAhead,
    CacheLimit,
    ClearCache,
    RemoveDownloads,
    OpenDownloads,
    Accent,
    Images,
    Visualizer,
    BarViz,
    ShowQuality,
    StartPage,
    HideExplicit,
    ClearRecents,
    SignOut,
    ShowLog,
    ResetAll,
}

pub enum SRow {
    Header(String),
    Account(String, String),
    Nav(SPage, String),
    Toggle(SKey, &'static str, bool, Option<String>),
    Choice(SKey, &'static str, String, Option<String>),
    Slider(SKey, String, f32, String),
    Action(SKey, String, bool, Option<String>),
    Info(&'static str, String),
    Note(String),
    EqGraph,
}

impl SRow {
    pub fn selectable(&self) -> bool {
        !matches!(self, SRow::Header(_) | SRow::Info(..) | SRow::Note(_) | SRow::EqGraph)
    }
}

fn db(v: f32) -> String {
    if v.abs() < 0.05 { "0.0 dB".into() } else { format!("{v:+.1} dB") }
}

fn step_in<T: PartialEq + Copy>(all: &[T], cur: T, dir: i32) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    let n = (i + dir).rem_euclid(all.len() as i32) as usize;
    all[n]
}

impl App {
    pub fn settings_rows(&self, page: SPage) -> Vec<SRow> {
        let s = &self.cfg.settings;
        let mut r = Vec::new();
        match page {
            SPage::Root => {
                let host = self
                    .client
                    .as_ref()
                    .map(|c| c.server().trim_start_matches("https://").trim_start_matches("http://").to_string())
                    .unwrap_or_default();
                r.push(SRow::Account(self.cfg.username.clone(), host));
                r.push(SRow::Header("Listening".into()));
                let playback = if s.crossfade > 0 {
                    format!("Crossfade {}s", s.crossfade)
                } else if s.gapless {
                    "Gapless".into()
                } else {
                    "Crossfade off".into()
                };
                r.push(SRow::Nav(SPage::Playback, playback));
                r.push(SRow::Nav(
                    SPage::Equalizer,
                    if s.eq_enabled { preset_name(&s.eq_gains).into() } else { "Off".into() },
                ));
                r.push(SRow::Nav(SPage::Lyrics, s.lyrics_source.label().into()));
                r.push(SRow::Nav(SPage::Quality, format!("Streaming {}", s.streaming_quality.short())));
                r.push(SRow::Header("Downloads and offline".into()));
                r.push(SRow::Toggle(
                    SKey::OfflineMode,
                    "Offline Mode",
                    s.offline_mode,
                    Some("Only plays downloaded or cached songs. Nothing streams over the network.".into()),
                ));
                let n = self.downloads.items.len();
                r.push(SRow::Nav(
                    SPage::Downloads,
                    if n == 0 { "No downloads".into() } else { format!("{n} song{} downloaded", if n == 1 { "" } else { "s" }) },
                ));
                r.push(SRow::Header("Look and feel".into()));
                r.push(SRow::Nav(SPage::Appearance, s.accent.label().into()));
                r.push(SRow::Nav(SPage::Player, format!("Visualizer {}", s.visualizer.label())));
                r.push(SRow::Nav(SPage::HomeLibrary, format!("Opens to {}", s.start_page.label())));
                r.push(SRow::Header("Library".into()));
                r.push(SRow::Nav(SPage::SearchContent, if s.hide_explicit { "Explicit hidden".into() } else { "Everything shown".into() }));
                r.push(SRow::Header(String::new()));
                r.push(SRow::Nav(SPage::About, env!("CARGO_PKG_VERSION").into()));
            }
            SPage::Account => {
                r.push(SRow::Info("Server", self.client.as_ref().map(|c| c.server().to_string()).unwrap_or_default()));
                r.push(SRow::Info("Username", self.cfg.username.clone()));
                r.push(SRow::Note("The password is not stored. The config file keeps the Subsonic salt and token.".into()));
                r.push(SRow::Info("Config file", crate::config::Config::path().display().to_string()));
                r.push(SRow::Action(SKey::SignOut, "Sign Out".into(), true, None));
            }
            SPage::Playback => {
                r.push(SRow::Header("Shuffle".into()));
                r.push(SRow::Choice(
                    SKey::Shuffle,
                    "Shuffle",
                    if s.shuffle_mode == ShuffleMode::FewerRepeats { "Fewer Repeats".into() } else { "Standard".into() },
                    Some("Fewer Repeats keeps recently played songs from coming back right away. Standard is fully random.".into()),
                ));
                r.push(SRow::Header("Transitions".into()));
                r.push(SRow::Slider(
                    SKey::Crossfade,
                    "Crossfade".into(),
                    s.crossfade as f32 / 12.0,
                    if s.crossfade == 0 { "Off".into() } else { format!("{}s", s.crossfade) },
                ));
                r.push(SRow::Toggle(
                    SKey::Gapless,
                    "Gapless Playback",
                    s.gapless,
                    (s.crossfade > 0).then(|| "Off while Crossfade is active.".to_string()),
                ));
                r.push(SRow::Toggle(
                    SKey::Autoplay,
                    "Autoplay",
                    s.autoplay,
                    Some("When the queue ends and Repeat is off, keep going with songs like the last one.".into()),
                ));
                r.push(SRow::Header("Volume".into()));
                r.push(SRow::Choice(
                    SKey::ReplayGain,
                    "ReplayGain",
                    s.replay_gain.label().into(),
                    Some("Track turns every song to the same loudness. Album keeps an album's quiet and loud songs in balance. Songs with no ReplayGain tags play as they are.".into()),
                ));
                if s.replay_gain != ReplayGain::Off {
                    r.push(SRow::Slider(
                        SKey::RgPreamp,
                        "Preamp".into(),
                        (s.replay_gain_preamp + 15.0) / 30.0,
                        db(s.replay_gain_preamp),
                    ));
                    r.push(SRow::Toggle(
                        SKey::PreventClipping,
                        "Prevent Clipping",
                        s.prevent_clipping,
                        Some("Holds a boost back so a song's loudest moment stays clean.".into()),
                    ));
                }
                r.push(SRow::Header("Behavior".into()));
                r.push(SRow::Toggle(
                    SKey::RestoreQueue,
                    "Restore Queue on Launch",
                    s.restore_queue,
                    Some("Restore the server's saved queue and position, paused and ready to continue.".into()),
                ));
            }
            SPage::Equalizer => {
                r.push(SRow::Toggle(SKey::EqEnabled, "Equalizer", s.eq_enabled, None));
                r.push(SRow::EqGraph);
                r.push(SRow::Choice(SKey::EqPreset, "Preset", preset_name(&s.eq_gains).into(), None));
                r.push(SRow::Slider(SKey::EqPreamp, "Preamp".into(), (s.eq_preamp + 12.0) / 24.0, db(s.eq_preamp)));
                r.push(SRow::Header("Bands".into()));
                for (i, hz) in EQ_BANDS.iter().enumerate() {
                    let label = if *hz >= 1000.0 { format!("{} kHz", hz / 1000.0) } else { format!("{hz} Hz") };
                    r.push(SRow::Slider(
                        SKey::EqBand(i),
                        label,
                        (s.eq_gains[i] + EQ_MAX_DB) / (2.0 * EQ_MAX_DB),
                        db(s.eq_gains[i]),
                    ));
                }
                r.push(SRow::Note("← → change the selected band by 1 dB. Drag a slider with the mouse.".into()));
            }
            SPage::Lyrics => {
                r.push(SRow::Choice(
                    SKey::LyricsSource,
                    "Lyrics Source",
                    s.lyrics_source.label().into(),
                    Some("LRCLIB is a free public lyrics database. It only needs artist and title, and does not use your server account.".into()),
                ));
                r.push(SRow::Toggle(
                    SKey::LyricsScroll,
                    "Auto-scroll Synced Lyrics",
                    s.lyrics_auto_scroll,
                    Some("Keeps the line that's playing in view.".into()),
                ));
            }
            SPage::Quality => {
                r.push(SRow::Choice(
                    SKey::StreamQuality,
                    "Streaming",
                    s.streaming_quality.label().into(),
                    Some("Original plays the file as stored. The others ask the server for an MP3 at that bitrate.".into()),
                ));
                r.push(SRow::Toggle(
                    SKey::LosslessOnly,
                    "Transcode Lossless Only",
                    s.transcode_lossless_only,
                    Some("Only FLAC, ALAC, WAV and other lossless files get transcoded. MP3, AAC and Ogg play as stored.".into()),
                ));
            }
            SPage::Downloads => {
                r.push(SRow::Header("Downloads".into()));
                r.push(SRow::Info(
                    "Downloaded",
                    format!("{} songs · {}", self.downloads.items.len(), fmt_bytes(self.downloads.bytes())),
                ));
                r.push(SRow::Action(SKey::OpenDownloads, "Manage Downloads".into(), false, None));
                r.push(SRow::Choice(
                    SKey::DownloadQuality,
                    "Download Quality",
                    s.download_quality.label().into(),
                    Some("Used for future downloads. Stored files keep the quality they were downloaded at.".into()),
                ));
                r.push(SRow::Action(SKey::RemoveDownloads, "Remove All Downloads".into(), true, None));
                r.push(SRow::Header("Cache".into()));
                let (bytes, files) = store::cache_usage();
                r.push(SRow::Info("Auto-cache used", format!("{} in {files} files, of {}", fmt_bytes(bytes), mb_label(s.cache_limit_mb))));
                r.push(SRow::Choice(
                    SKey::CacheAhead,
                    "Cache Ahead",
                    match s.cache_ahead {
                        0 => "Off".into(),
                        1 => "Next song".into(),
                        n => format!("Next {n} songs"),
                    },
                    Some("Fetches upcoming songs into the cache once the current one has downloaded.".into()),
                ));
                r.push(SRow::Choice(SKey::CacheLimit, "Automatic Cache Limit", mb_label(s.cache_limit_mb), None));
                r.push(SRow::Action(SKey::ClearCache, "Clear Cache".into(), false, None));
            }
            SPage::Appearance => {
                r.push(SRow::Choice(SKey::Accent, "Accent color", s.accent.label().into(), None));
                let art = match self.art.as_ref().map(|a| a.mode) {
                    Some(crate::art::Mode::Auto) => format!("Best available ({})", self.art.as_ref().map(|a| a.protocol_name()).unwrap_or("")),
                    Some(crate::art::Mode::Halfblocks) => "Pixel blocks".into(),
                    _ => "Off".into(),
                };
                r.push(SRow::Choice(
                    SKey::Images,
                    "Cover art",
                    art,
                    Some("Best available uses Kitty, iTerm2 or Sixel images when the terminal has them, and pixel blocks otherwise.".into()),
                ));
            }
            SPage::Player => {
                r.push(SRow::Choice(
                    SKey::Visualizer,
                    "Visualizer",
                    s.visualizer.label().into(),
                    Some("Shown in the full screen player. The bars follow the music that's playing.".into()),
                ));
                r.push(SRow::Toggle(SKey::BarViz, "Visualizer in the player bar", s.bar_visualizer, None));
                r.push(SRow::Toggle(SKey::ShowQuality, "Show Now Playing Audio Quality", s.show_audio_quality, None));
            }
            SPage::HomeLibrary => {
                r.push(SRow::Choice(SKey::StartPage, "Opens to", s.start_page.label().into(), None));
            }
            SPage::SearchContent => {
                r.push(SRow::Toggle(
                    SKey::HideExplicit,
                    "Hide Explicit Content",
                    s.hide_explicit,
                    Some("Hides songs your server marks explicit, and keeps them out of Play and Shuffle.".into()),
                ));
                r.push(SRow::Action(
                    SKey::ClearRecents,
                    "Clear Recent Searches".into(),
                    false,
                    Some(format!("{} saved", s.recent_searches.len())),
                ));
            }
            SPage::About => {
                r.push(SRow::Info("Version", env!("CARGO_PKG_VERSION").into()));
                r.push(SRow::Info("Log file", crate::log::path().display().to_string()));
                #[cfg(target_os = "macos")]
                r.push(SRow::Action(SKey::ShowLog, "Show the log in Finder".into(), false, None));
                #[cfg(windows)]
                r.push(SRow::Action(SKey::ShowLog, "Show the log in File Explorer".into(), false, None));
                r.push(SRow::Header("Recent log lines".into()));
                for l in crate::log::tail(8) {
                    r.push(SRow::Note(l));
                }
                r.push(SRow::Header(String::new()));
                r.push(SRow::Action(SKey::ResetAll, "Reset All Settings".into(), true, None));
            }
        }
        r
    }

    pub fn settings_selectable(&self, page: SPage) -> Vec<usize> {
        self.settings_rows(page)
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selectable())
            .map(|(i, _)| i)
            .collect()
    }

    fn settings_current(&self) -> Option<(SPage, usize)> {
        match self.top_kind() {
            Some(Kind::Settings { page, sel, .. }) => Some((*page, *sel)),
            _ => None,
        }
    }

    /// Puts the cursor on the first row that can be used.
    pub fn settings_fix_sel(&mut self) {
        let Some((page, sel)) = self.settings_current() else { return };
        let rows = self.settings_selectable(page);
        if rows.contains(&sel) {
            return;
        }
        let pick = rows.iter().copied().find(|r| *r >= sel).or(rows.last().copied()).unwrap_or(0);
        if let Some(View { kind: Kind::Settings { sel, .. }, .. }) = self.stack.last_mut() {
            *sel = pick;
        }
    }

    pub fn settings_activate(&mut self) {
        let Some((page, sel)) = self.settings_current() else { return };
        let rows = self.settings_rows(page);
        let Some(row) = rows.get(sel) else { return };
        match row {
            SRow::Account(..) => self.open_settings(SPage::Account),
            SRow::Nav(p, _) => {
                let p = *p;
                self.open_settings(p);
                self.settings_fix_sel();
            }
            SRow::Toggle(..) => self.settings_adjust(0),
            SRow::Choice(..) => self.settings_adjust(1),
            SRow::Action(k, ..) => {
                let k = *k;
                self.settings_action(k);
            }
            _ => {}
        }
    }

    fn settings_action(&mut self, k: SKey) {
        let destructive = matches!(k, SKey::RemoveDownloads | SKey::ResetAll | SKey::SignOut | SKey::ClearCache);
        if destructive && self.confirm.map(|c| c.0) != Some(k) {
            self.confirm = Some((k, std::time::Instant::now()));
            self.toast("Press Enter again to confirm.");
            return;
        }
        self.confirm = None;
        match k {
            SKey::ClearCache => {
                store::clear_cache();
                self.toast("Cleared the cache");
            }
            SKey::RemoveDownloads => {
                self.downloads.remove_all();
                self.downloaded_ids.clear();
                self.toast("Removed all downloads");
            }
            SKey::OpenDownloads => self.open_nav(super::NAV_DOWNLOADS),
            SKey::ClearRecents => {
                self.cfg.settings.recent_searches.clear();
                self.cfg.save();
                self.toast("Cleared recent searches");
            }
            SKey::SignOut => self.run(super::Action::SignOut),
            SKey::ShowLog => {
                #[cfg(windows)]
                let _ = std::process::Command::new("explorer")
                    .arg(format!("/select,{}", crate::log::path().display()))
                    .spawn();
                #[cfg(not(windows))]
                let _ = std::process::Command::new("open").arg("-R").arg(crate::log::path()).spawn();
            }
            SKey::ResetAll => {
                let recents = std::mem::take(&mut self.cfg.settings.recent_searches);
                self.cfg.settings = Settings { recent_searches: recents, ..Default::default() };
                self.apply_settings();
                self.cfg.save();
                self.toast("Settings are back to their defaults");
            }
            _ => {}
        }
    }

    /// Left and right on a row: step a choice or slider, or set a switch.
    pub fn settings_adjust(&mut self, dir: i32) {
        let Some((page, sel)) = self.settings_current() else { return };
        let rows = self.settings_rows(page);
        let key = match rows.get(sel) {
            Some(SRow::Toggle(k, ..)) | Some(SRow::Choice(k, ..)) | Some(SRow::Slider(k, ..)) => *k,
            Some(SRow::Nav(..)) | Some(SRow::Account(..)) if dir > 0 => return self.settings_activate(),
            _ => return,
        };
        self.change(key, dir);
        self.apply_settings();
        self.cfg.save();
        self.settings_fix_sel();
    }

    fn change(&mut self, key: SKey, dir: i32) {
        let s = &mut self.cfg.settings;
        // Enter flips a switch. Right turns it on and left turns it off.
        let flip = |b: &mut bool| *b = if dir == 0 { !*b } else { dir > 0 };
        let dir = if dir == 0 { 1 } else { dir };
        match key {
            SKey::OfflineMode => {
                flip(&mut s.offline_mode);
                let on = s.offline_mode;
                self.toast(if on { "Offline Mode on: only downloads and cached songs play" } else { "Offline Mode off" });
            }
            SKey::Shuffle => {
                s.shuffle_mode = if s.shuffle_mode == ShuffleMode::FewerRepeats { ShuffleMode::Standard } else { ShuffleMode::FewerRepeats }
            }
            SKey::Crossfade => s.crossfade = (s.crossfade as i32 + dir).clamp(0, 12) as u32,
            SKey::Gapless => {
                if s.crossfade == 0 {
                    flip(&mut s.gapless)
                }
            }
            SKey::Autoplay => flip(&mut s.autoplay),
            SKey::ReplayGain => s.replay_gain = step_in(&[ReplayGain::Off, ReplayGain::Track, ReplayGain::Album], s.replay_gain, dir),
            SKey::RgPreamp => s.replay_gain_preamp = (s.replay_gain_preamp + 0.5 * dir as f32).clamp(-15.0, 15.0),
            SKey::PreventClipping => flip(&mut s.prevent_clipping),
            SKey::RestoreQueue => flip(&mut s.restore_queue),
            SKey::EqEnabled => flip(&mut s.eq_enabled),
            SKey::EqPreset => {
                let i = EQ_PRESETS.iter().position(|(n, _)| *n == preset_name(&s.eq_gains)).map(|i| i as i32).unwrap_or(-1);
                let n = (i + dir).rem_euclid(EQ_PRESETS.len() as i32) as usize;
                s.eq_gains = EQ_PRESETS[n].1;
                s.eq_enabled = true;
            }
            SKey::EqPreamp => s.eq_preamp = (s.eq_preamp + 0.5 * dir as f32).clamp(-12.0, 12.0),
            SKey::EqBand(i) => {
                s.eq_gains[i] = (s.eq_gains[i] + dir as f32).clamp(-EQ_MAX_DB, EQ_MAX_DB);
                s.eq_enabled = true;
            }
            SKey::LyricsSource => {
                s.lyrics_source = step_in(&LyricsSource::ALL, s.lyrics_source, dir);
                self.lyrics = None;
            }
            SKey::LyricsScroll => flip(&mut s.lyrics_auto_scroll),
            SKey::StreamQuality => s.streaming_quality = step_in(&Quality::ALL, s.streaming_quality, dir),
            SKey::LosslessOnly => flip(&mut s.transcode_lossless_only),
            SKey::DownloadQuality => s.download_quality = step_in(&Quality::ALL, s.download_quality, dir),
            SKey::CacheAhead => s.cache_ahead = (s.cache_ahead as i32 + dir).rem_euclid(6) as u32,
            SKey::CacheLimit => s.cache_limit_mb = step_in(&CACHE_LIMITS_MB, s.cache_limit_mb, dir),
            SKey::Accent => s.accent = step_in(&Accent::ALL, s.accent, dir),
            SKey::Images => {
                if let Some(a) = &mut self.art {
                    use crate::art::Mode;
                    let m = step_in(&[Mode::Auto, Mode::Halfblocks, Mode::Off], a.mode, dir);
                    a.set_mode(m);
                    self.cfg.images = m.as_str().into();
                }
            }
            SKey::Visualizer => s.visualizer = step_in(&Visualizer::ALL, s.visualizer, dir),
            SKey::BarViz => flip(&mut s.bar_visualizer),
            SKey::ShowQuality => flip(&mut s.show_audio_quality),
            SKey::StartPage => s.start_page = step_in(&StartPage::ALL, s.start_page, dir),
            SKey::HideExplicit => flip(&mut s.hide_explicit),
            _ => {}
        }
    }

    /// Sets a slider row from a 0..1 position, as the mouse drags it.
    pub fn settings_set_frac(&mut self, row: usize, frac: f32) {
        let Some((page, _)) = self.settings_current() else { return };
        let rows = self.settings_rows(page);
        let Some(SRow::Slider(key, ..)) = rows.get(row) else { return };
        let f = frac.clamp(0.0, 1.0);
        let s = &mut self.cfg.settings;
        match key {
            SKey::Crossfade => s.crossfade = (f * 12.0).round() as u32,
            SKey::RgPreamp => s.replay_gain_preamp = ((f * 30.0 - 15.0) * 2.0).round() / 2.0,
            SKey::EqPreamp => s.eq_preamp = ((f * 24.0 - 12.0) * 2.0).round() / 2.0,
            SKey::EqBand(i) => {
                s.eq_gains[*i] = (f * 2.0 * EQ_MAX_DB - EQ_MAX_DB).round();
                s.eq_enabled = true;
            }
            _ => return,
        }
        self.apply_settings();
    }
}
