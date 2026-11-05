//! Cover art.
//!
//! Views ask for `(cover id, columns, rows)` while drawing. A miss queues a
//! job; worker threads fetch the image (disk cache first), fit it to the cell
//! box, and encode it for the terminal's graphics protocol. The view draws a
//! generated placeholder until the result lands, like the iOS app's
//! `Palette` art. Lanczos keeps the downscale sharp, so half blocks read as
//! crisp pixels rather than a blur.

use crate::api::Client;
use crossbeam_channel::{Receiver, Sender, unbounded};
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::{Resize, picker::Picker, protocol::Protocol};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Auto,
    Halfblocks,
    Off,
}

impl Mode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "halfblocks" | "blocks" => Mode::Halfblocks,
            "off" | "none" => Mode::Off,
            _ => Mode::Auto,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Halfblocks => "halfblocks",
            Mode::Off => "off",
        }
    }
}

type Key = (String, u16, u16, u64);

struct Job {
    key: Key,
    picker: Picker,
}

pub struct Art {
    detected: Picker,
    pub mode: Mode,
    epoch: u64,
    ready: HashMap<Key, Protocol>,
    failed: HashSet<String>,
    pending: HashSet<Key>,
    jobs: Sender<Job>,
    results: Receiver<(Key, Option<Protocol>)>,
}

impl Art {
    pub fn new(client: Client, detected: Picker, mode: Mode) -> Self {
        let (jobs, job_rx) = unbounded::<Job>();
        let (res_tx, results) = unbounded();
        let decoded: Arc<Mutex<HashMap<String, Arc<DynamicImage>>>> = Default::default();
        let dir = crate::config::cache_dir().join("covers");
        let _ = std::fs::create_dir_all(&dir);
        for _ in 0..4 {
            let rx = job_rx.clone();
            let tx = res_tx.clone();
            let client = client.clone();
            let decoded = decoded.clone();
            let dir = dir.clone();
            std::thread::spawn(move || {
                while let Ok(job) = rx.recv() {
                    let out = render(&client, &decoded, &dir, &job);
                    if tx.send((job.key, out)).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            detected,
            mode,
            epoch: 0,
            ready: HashMap::new(),
            failed: HashSet::new(),
            pending: HashSet::new(),
            jobs,
            results,
        }
    }

    pub fn protocol_name(&self) -> &'static str {
        match self.mode {
            Mode::Off => "off",
            Mode::Halfblocks => "half-blocks",
            Mode::Auto => match self.detected.protocol_type() {
                ratatui_image::picker::ProtocolType::Halfblocks => "half-blocks",
                ratatui_image::picker::ProtocolType::Sixel => "Sixel",
                ratatui_image::picker::ProtocolType::Kitty => "Kitty",
                ratatui_image::picker::ProtocolType::Iterm2 => "iTerm2",
            },
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if mode != self.mode {
            self.mode = mode;
            self.epoch += 1;
            self.ready.clear();
            self.pending.clear();
        }
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Auto => Mode::Halfblocks,
            Mode::Halfblocks => Mode::Off,
            Mode::Off => Mode::Auto,
        };
        self.epoch += 1;
        self.ready.clear();
        self.pending.clear();
    }

    /// Pulls finished work. Returns true when something new can be drawn.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, proto)) = self.results.try_recv() {
            self.pending.remove(&key);
            if key.3 != self.epoch {
                continue;
            }
            match proto {
                Some(p) => {
                    if self.ready.len() > 400 {
                        self.ready.clear();
                    }
                    self.ready.insert(key, p);
                }
                None => {
                    self.failed.insert(key.0);
                }
            }
            any = true;
        }
        any
    }

    pub fn get(&mut self, id: &str, w: u16, h: u16) -> Option<&Protocol> {
        if self.mode == Mode::Off || w == 0 || h == 0 || self.failed.contains(id) {
            return None;
        }
        let key: Key = (id.to_string(), w, h, self.epoch);
        if self.ready.contains_key(&key) {
            return self.ready.get(&key);
        }
        if self.pending.insert(key.clone()) {
            let picker = match self.mode {
                Mode::Halfblocks => Picker::halfblocks(),
                _ => self.detected.clone(),
            };
            let _ = self.jobs.send(Job { key, picker });
        }
        None
    }
}

fn cache_path(dir: &std::path::Path, id: &str) -> PathBuf {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    dir.join(format!("{safe}.img"))
}

fn render(
    client: &Client,
    decoded: &Mutex<HashMap<String, Arc<DynamicImage>>>,
    dir: &std::path::Path,
    job: &Job,
) -> Option<Protocol> {
    let (id, w, h, _) = &job.key;
    let img = {
        let hit = decoded.lock().unwrap().get(id).cloned();
        match hit {
            Some(i) => i,
            None => {
                let path = cache_path(dir, id);
                let bytes = match std::fs::read(&path) {
                    Ok(b) if !b.is_empty() => b,
                    _ => {
                        let b = client.fetch_bytes(&client.cover_url(id, 600), 16 << 20).ok()?;
                        let _ = std::fs::write(&path, &b);
                        b
                    }
                };
                let i = Arc::new(image::load_from_memory(&bytes).ok()?);
                let mut map = decoded.lock().unwrap();
                if map.len() > 120 {
                    map.clear();
                }
                map.insert(id.clone(), i.clone());
                i
            }
        }
    };
    job.picker
        .new_protocol(
            (*img).clone(),
            Size::new(*w, *h),
            Resize::Scale(Some(image::imageops::FilterType::Lanczos3)),
        )
        .ok()
}
