//! Audio on disk: the automatic cache and downloads.
//!
//! The cache holds songs that finished streaming and songs fetched ahead of
//! the queue. It is trimmed oldest first to the size limit. Downloads are kept
//! until removed, and `downloads.json` remembers their metadata so they play
//! with no server at all.
//!
//! `ext-` ids from octo-fiesta can change between sessions, so a download is also
//! found by artist, title and album when its id has changed.

use crate::api::Song;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

static CACHE_LIMIT: AtomicU64 = AtomicU64::new(2048 * 1024 * 1024);

pub fn set_cache_limit_mb(mb: u64) {
    CACHE_LIMIT.store(mb * 1024 * 1024, Ordering::Relaxed);
}

pub fn cache_dir() -> PathBuf {
    crate::config::cache_dir().join("audio")
}

pub fn downloads_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SPLYNT_DATA") {
        return PathBuf::from(p).join("downloads");
    }
    dirs::data_dir()
        .unwrap_or_else(|| crate::config::cache_dir())
        .join("splynt")
        .join("downloads")
}

fn key(id: &str) -> String {
    format!("{:x}", md5::compute(id))
}

pub fn ext_for(song: &Song, transcoded: Option<&str>) -> String {
    transcoded
        .map(|s| s.to_string())
        .or_else(|| song.suffix.clone())
        .unwrap_or_else(|| "bin".into())
        .to_ascii_lowercase()
}

pub fn meta_key(s: &Song) -> String {
    format!("{}|{}|{}", s.artist.to_lowercase(), s.title.to_lowercase(), s.album.to_lowercase())
}

/// A cached copy of `id`, touched so the trim keeps it.
pub fn cached(id: &str) -> Option<PathBuf> {
    let dir = cache_dir();
    let k = key(id);
    let rd = std::fs::read_dir(&dir).ok()?;
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&k) && !name.ends_with(".part") {
            let p = e.path();
            let _ = std::fs::File::options().append(true).open(&p).and_then(|f| f.set_modified(SystemTime::now()));
            return Some(p);
        }
    }
    None
}

pub fn cache_path(id: &str, ext: &str) -> PathBuf {
    cache_dir().join(format!("{}.{ext}", key(id)))
}

/// Writes `bytes` into the cache, then trims it.
pub fn put_cache(id: &str, ext: &str, bytes: &[u8]) {
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let dest = cache_path(id, ext);
    let tmp = dest.with_extension(format!("{ext}.part"));
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, &dest).is_ok() {
        trim_cache(Some(&dest));
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

pub fn trim_cache(keep: Option<&Path>) {
    let limit = CACHE_LIMIT.load(Ordering::Relaxed);
    let Ok(rd) = std::fs::read_dir(cache_dir()) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, p) in files {
        if total <= limit {
            break;
        }
        if Some(p.as_path()) == keep {
            continue;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

pub fn cache_usage() -> (u64, usize) {
    dir_usage(&cache_dir())
}

fn dir_usage(dir: &Path) -> (u64, usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return (0, 0) };
    rd.flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .fold((0, 0), |(b, n), m| (b + m.len(), n + 1))
}

pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

// ---------------------------------------------------------------- downloads

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Download {
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
    pub suffix: String,
    pub bytes: u64,
    pub file: String,
    pub quality: String,
}

impl Download {
    pub fn song(&self) -> Song {
        Song {
            id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            artist_id: self.artist_id.clone(),
            album: self.album.clone(),
            album_id: self.album_id.clone(),
            cover: self.cover.clone(),
            duration: self.duration,
            track: self.track,
            year: self.year,
            suffix: Some(self.suffix.clone()),
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct Downloads {
    pub items: Vec<Download>,
    by_id: HashMap<String, usize>,
    by_meta: HashMap<String, usize>,
}

impl Downloads {
    fn index_path() -> PathBuf {
        downloads_dir().join("downloads.json")
    }

    pub fn load() -> Self {
        let items: Vec<Download> = std::fs::read_to_string(Self::index_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Drop entries whose file went missing.
        let items = items.into_iter().filter(|d| downloads_dir().join(&d.file).exists()).collect();
        let mut d = Self { items, ..Default::default() };
        d.reindex();
        d
    }

    fn reindex(&mut self) {
        self.by_id.clear();
        self.by_meta.clear();
        for (i, d) in self.items.iter().enumerate() {
            self.by_id.insert(d.id.clone(), i);
            self.by_meta.insert(meta_key(&d.song()), i);
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(downloads_dir());
        if let Ok(s) = serde_json::to_string_pretty(&self.items) {
            let tmp = Self::index_path().with_extension("json.part");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, Self::index_path());
            }
        }
    }

    pub fn find(&self, s: &Song) -> Option<&Download> {
        self.by_id
            .get(&s.id)
            .or_else(|| self.by_meta.get(&meta_key(s)))
            .and_then(|&i| self.items.get(i))
    }

    pub fn has(&self, s: &Song) -> bool {
        self.find(s).is_some()
    }

    pub fn file_for(&self, s: &Song) -> Option<PathBuf> {
        self.find(s).map(|d| downloads_dir().join(&d.file))
    }

    pub fn add(&mut self, d: Download) {
        if let Some(&i) = self.by_id.get(&d.id) {
            self.items[i] = d;
        } else {
            self.items.push(d);
        }
        self.reindex();
        self.save();
    }

    pub fn remove(&mut self, s: &Song) -> bool {
        let Some(d) = self.find(s).cloned() else { return false };
        let _ = std::fs::remove_file(downloads_dir().join(&d.file));
        self.items.retain(|x| x.id != d.id);
        self.reindex();
        self.save();
        true
    }

    pub fn remove_all(&mut self) {
        for d in &self.items {
            let _ = std::fs::remove_file(downloads_dir().join(&d.file));
        }
        self.items.clear();
        self.reindex();
        self.save();
    }

    pub fn bytes(&self) -> u64 {
        self.items.iter().map(|d| d.bytes).sum()
    }

    pub fn songs(&self) -> Vec<Song> {
        let mut v: Vec<Song> = self.items.iter().map(|d| d.song()).collect();
        v.sort_by(|a, b| {
            (a.artist.to_lowercase(), a.album.to_lowercase(), a.track.unwrap_or(0))
                .cmp(&(b.artist.to_lowercase(), b.album.to_lowercase(), b.track.unwrap_or(0)))
        });
        v
    }
}

/// Download jobs run one at a time on their own thread.
pub struct Job {
    pub song: Song,
    pub url: String,
    pub ext: String,
    pub quality: String,
}

#[derive(Default)]
pub struct Progress {
    pub queued: usize,
    pub done: usize,
    pub failed: usize,
    pub current: Option<(String, u64, Option<u64>)>,
}

pub type SharedProgress = Arc<Mutex<Progress>>;

pub fn fetch_to(client: &crate::api::Client, url: &str, dest: &Path, progress: Option<&SharedProgress>) -> Result<u64, String> {
    use std::io::{Read, Write};
    let (mut r, len) = client.open_stream(url)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = dest.with_extension("part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.to_string());
            }
        };
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
        if let Some(p) = progress {
            if let Ok(mut g) = p.lock() {
                if let Some(c) = g.current.as_mut() {
                    c.1 = total;
                    c.2 = len;
                }
            }
        }
    }
    if let Some(l) = len {
        if total < l {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("download stopped at {total} of {l} bytes"));
        }
    }
    drop(f);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok(total)
}
