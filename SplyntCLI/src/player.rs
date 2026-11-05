//! Audio output.
//!
//! Three kinds of thread take part:
//!
//! - A download thread per streamed track fills a memory buffer, reconnects
//!   with a `Range` request if the connection drops, and copies the finished
//!   file into the cache.
//! - A decode thread per track turns that buffer (or a file on disk) into
//!   stereo samples at the output rate and hands them over in chunks.
//! - The output callback runs `Deck`, the one source the output device ever
//!   sees. It never blocks: when a chunk is late it plays silence. It applies
//!   ReplayGain and the equalizer, crossfades or joins tracks back to back,
//!   and keeps the last few thousand samples for the visualizer.
//!
//! The engine thread in between takes commands from the app and publishes a
//! `Status` and a spectrum.

use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded, unbounded};
use rodio::source::UniformSourceIterator;
use rodio::{Decoder, MixerDeviceSink, Player, Source};
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const BANDS: usize = 24;
const CHUNK: usize = 4096;
const CHUNKS_AHEAD: usize = 24;

pub type Reopen = Box<dyn Fn(u64) -> Result<(Box<dyn Read + Send>, bool), String> + Send>;

pub enum InputSource {
    File(PathBuf),
    Stream {
        reader: Box<dyn Read + Send>,
        len: Option<u64>,
        /// Reopens the stream at a byte offset. The bool says whether the
        /// server honoured the range.
        reopen: Option<Reopen>,
        /// Song id and file extension to cache the finished download under.
        cache: Option<(String, String)>,
    },
}

pub struct Input {
    pub source: InputSource,
    pub hint: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EqConfig {
    pub enabled: bool,
    pub preamp_db: f32,
    pub gains: [f32; 10],
}

pub enum Cmd {
    Load { input: Input, generation: u64, start_at: f64, paused: bool, gain: f32 },
    Preload { input: Input, generation: u64, gain: f32 },
    ClearPreload,
    Toggle,
    Stop,
    Seek(f64),
    Volume(f32),
    Eq(EqConfig),
    Transitions { crossfade: f32, gapless: bool },
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub generation: u64,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub loading: bool,
    pub playing: bool,
    /// Set once, when the track for `generation` runs out with nothing after it.
    pub ended: bool,
    pub error: Option<String>,
    pub seekable: bool,
    pub buffered: f64,
    /// The generation waiting to follow the current one.
    pub preloaded: Option<u64>,
}

pub struct Engine {
    tx: Sender<Cmd>,
    pub status: Arc<Mutex<Status>>,
    pub spectrum: Arc<Mutex<[f32; BANDS]>>,
}

impl Engine {
    pub fn start(volume: f32) -> Result<Self, String> {
        let (tx, rx) = unbounded();
        let status = Arc::new(Mutex::new(Status { paused: true, ..Default::default() }));
        let spectrum = Arc::new(Mutex::new([0.0; BANDS]));
        let (st, sp) = (status.clone(), spectrum.clone());
        let (ready_tx, ready_rx) = unbounded::<Result<(), String>>();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(rx, st, sp, volume, ready_tx))
            .map_err(|e| e.to_string())?;
        ready_rx.recv().map_err(|e| e.to_string())??;
        Ok(Self { tx, status, spectrum })
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn spectrum(&self) -> [f32; BANDS] {
        *self.spectrum.lock().unwrap()
    }
}

// ---------------------------------------------------------------- download buffer

struct Shared {
    data: Mutex<Vec<u8>>,
    cond: Condvar,
    done: AtomicBool,
    cancelled: AtomicBool,
    total: Option<u64>,
}

struct StreamReader {
    shared: Arc<Shared>,
    pos: u64,
}

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut data = self.shared.data.lock().unwrap();
        loop {
            if self.shared.cancelled.load(Ordering::SeqCst) {
                return Ok(0);
            }
            let have = data.len() as u64;
            if self.pos < have {
                let n = ((have - self.pos) as usize).min(buf.len());
                let start = self.pos as usize;
                buf[..n].copy_from_slice(&data[start..start + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if self.shared.done.load(Ordering::SeqCst) {
                return Ok(0);
            }
            data = self.shared.cond.wait_timeout(data, Duration::from_millis(200)).unwrap().0;
        }
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let end = match self.shared.total {
            Some(t) => t as i64,
            None => {
                let mut data = self.shared.data.lock().unwrap();
                while !self.shared.done.load(Ordering::SeqCst) && !self.shared.cancelled.load(Ordering::SeqCst) {
                    data = self.shared.cond.wait_timeout(data, Duration::from_millis(200)).unwrap().0;
                }
                data.len() as i64
            }
        };
        let next = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => end + d,
        };
        if next < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

fn spawn_download(mut reader: Box<dyn Read + Send>, shared: Arc<Shared>, reopen: Option<Reopen>, cache: Option<(String, String)>) {
    std::thread::spawn(move || {
        let mut chunk = vec![0u8; 64 * 1024];
        let mut retries = 0u32;
        loop {
            if shared.cancelled.load(Ordering::SeqCst) {
                break;
            }
            match reader.read(&mut chunk) {
                Ok(0) => {
                    let have = shared.data.lock().unwrap().len() as u64;
                    match shared.total {
                        Some(t) if have < t => {}
                        _ => break,
                    }
                    if !reconnect(&shared, &reopen, &mut reader, &mut retries, "the stream closed early") {
                        break;
                    }
                }
                Ok(n) => {
                    retries = 0;
                    shared.data.lock().unwrap().extend_from_slice(&chunk[..n]);
                    shared.cond.notify_all();
                }
                Err(e) => {
                    if !reconnect(&shared, &reopen, &mut reader, &mut retries, &e.to_string()) {
                        break;
                    }
                }
            }
        }
        let cancelled = shared.cancelled.load(Ordering::SeqCst);
        shared.done.store(true, Ordering::SeqCst);
        shared.cond.notify_all();
        if let (Some((id, ext)), false) = (cache, cancelled) {
            let data = shared.data.lock().unwrap();
            let complete = shared.total.is_none_or(|t| data.len() as u64 >= t);
            if complete && !data.is_empty() {
                crate::store::put_cache(&id, &ext, &data);
            }
        }
    });
}

fn reconnect(shared: &Shared, reopen: &Option<Reopen>, reader: &mut Box<dyn Read + Send>, retries: &mut u32, why: &str) -> bool {
    let Some(reopen) = reopen else {
        crate::log::warn(format!("stream stopped: {why}"));
        return false;
    };
    while *retries < 5 && !shared.cancelled.load(Ordering::SeqCst) {
        *retries += 1;
        let wait = 500u64 << (*retries - 1);
        crate::log::warn(format!("stream stopped ({why}), reconnecting in {wait} ms, try {retries}"));
        std::thread::sleep(Duration::from_millis(wait));
        let have = shared.data.lock().unwrap().len() as u64;
        match reopen(have) {
            Ok((mut r, ranged)) => {
                if !ranged {
                    // The server sent the whole file again. Skip what we have.
                    let mut skip = have;
                    let mut buf = vec![0u8; 64 * 1024];
                    while skip > 0 {
                        match r.read(&mut buf[..(skip as usize).min(64 * 1024)]) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => skip -= n as u64,
                        }
                    }
                    if skip > 0 {
                        continue;
                    }
                }
                *reader = r;
                return true;
            }
            Err(e) => crate::log::warn(format!("reconnect failed: {e}")),
        }
    }
    false
}

// ---------------------------------------------------------------- decoding

enum Chunk {
    Data { epoch: u32, start: f64, dur: Option<f64>, samples: Vec<f32> },
    End { epoch: u32 },
}

enum DecCmd {
    Seek(f64),
}

#[derive(Default)]
struct Info {
    ready: bool,
    duration: Option<f64>,
    seekable: bool,
    error: Option<String>,
}

/// Whatever a decoder reads from: a file or the download buffer.
trait Media: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> Media for T {}

fn spawn_decoder(input: Input, rate: u32, start_at: f64, info: Arc<Mutex<Info>>, tx: Sender<Chunk>, ctrl: Receiver<DecCmd>, shared: Option<Arc<Shared>>) {
    std::thread::Builder::new()
        .name("decode".into())
        .spawn(move || {
            let hint = input.hint.clone();
            let built = match input.source {
                InputSource::File(path) => match std::fs::File::open(&path) {
                    Ok(f) => {
                        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                        let data: Box<dyn Media> = Box::new(f);
                        let mut b = Decoder::builder().with_data(data).with_byte_len(len).with_seekable(true);
                        if let Some(h) = hint.as_deref() {
                            b = b.with_hint(h);
                        }
                        b.build().map(|d| (d, true)).map_err(|e| e.to_string())
                    }
                    Err(e) => Err(e.to_string()),
                },
                InputSource::Stream { len, .. } => {
                    let sh = shared.clone().expect("stream without a buffer");
                    let data: Box<dyn Media> = Box::new(StreamReader { shared: sh, pos: 0 });
                    let mut b = Decoder::builder().with_data(data);
                    if let Some(l) = len {
                        b = b.with_byte_len(l).with_seekable(true);
                    } else {
                        b = b.with_seekable(false);
                    }
                    if let Some(h) = hint.as_deref() {
                        b = b.with_hint(h);
                    }
                    b.build().map(|d| (d, len.is_some())).map_err(|e| e.to_string())
                }
            };
            let (dec, seekable) = match built {
                Ok(x) => x,
                Err(e) => {
                    crate::log::error(format!("decode failed: {e}"));
                    let mut i = info.lock().unwrap();
                    i.error = Some(format!("Couldn't decode this track: {e}"));
                    i.ready = true;
                    drop(i);
                    let _ = tx.send(Chunk::End { epoch: 0 });
                    return;
                }
            };
            let dur = dec.total_duration().map(|d| d.as_secs_f64());
            {
                let mut i = info.lock().unwrap();
                i.duration = dur;
                i.seekable = seekable;
                i.ready = true;
            }
            let mut src = UniformSourceIterator::new(dec, NonZero::new(2).unwrap(), NonZero::new(rate).unwrap());
            let mut pos = 0.0f64;
            if start_at > 1.0 && seekable && src.try_seek(Duration::from_secs_f64(start_at)).is_ok() {
                pos = start_at;
            }
            let mut epoch = 0u32;
            let seek = |src: &mut UniformSourceIterator<Decoder<Box<dyn Media>>>, epoch: &mut u32, pos: &mut f64, t: f64| {
                *epoch += 1;
                let started = Instant::now();
                match src.try_seek(Duration::from_secs_f64(t.max(0.0))) {
                    Ok(()) => *pos = t.max(0.0),
                    Err(e) => crate::log::warn(format!("seek to {t:.1}s failed: {e}")),
                }
                if started.elapsed() > Duration::from_millis(250) {
                    crate::log::warn(format!("seek to {t:.1}s took {} ms", started.elapsed().as_millis()));
                }
            };
            'outer: loop {
                loop {
                    match ctrl.try_recv() {
                        Ok(DecCmd::Seek(t)) => seek(&mut src, &mut epoch, &mut pos, t),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
                let mut buf = Vec::with_capacity(CHUNK);
                for _ in 0..CHUNK {
                    match src.next() {
                        Some(s) => buf.push(s),
                        None => break,
                    }
                }
                if buf.len() % 2 == 1 {
                    buf.push(0.0);
                }
                let ended = buf.len() < CHUNK;
                if !buf.is_empty() {
                    let frames = buf.len() / 2;
                    let mut msg = Chunk::Data { epoch, start: pos, dur, samples: buf };
                    loop {
                        match tx.send_timeout(msg, Duration::from_millis(100)) {
                            Ok(()) => break,
                            Err(crossbeam_channel::SendTimeoutError::Timeout(m)) => {
                                msg = m;
                                match ctrl.try_recv() {
                                    Ok(DecCmd::Seek(t)) => {
                                        seek(&mut src, &mut epoch, &mut pos, t);
                                        continue 'outer;
                                    }
                                    Err(TryRecvError::Disconnected) => return,
                                    Err(TryRecvError::Empty) => {}
                                }
                            }
                            Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => return,
                        }
                    }
                    pos += frames as f64 / rate as f64;
                }
                if ended {
                    crate::log::info(format!("decoded to the end at {pos:.1}s"));
                    if tx.send(Chunk::End { epoch }).is_err() {
                        return;
                    }
                    // Stay around: a seek back can still restart decoding.
                    match ctrl.recv() {
                        Ok(DecCmd::Seek(t)) => seek(&mut src, &mut epoch, &mut pos, t),
                        Err(_) => return,
                    }
                }
            }
        })
        .expect("decode thread");
}

// ---------------------------------------------------------------- deck

struct DeckTrack {
    generation: u64,
    rx: Receiver<Chunk>,
    ctrl: Sender<DecCmd>,
    gain: f32,
    buf: Vec<f32>,
    idx: usize,
    epoch: u32,
    base: f64,
    played: u64,
    dur: Option<f64>,
    ended: bool,
    started: bool,
}

impl DeckTrack {
    /// One stereo frame, or None when no data is ready or the track is done.
    fn pull(&mut self, underrun: &mut bool) -> Option<(f32, f32)> {
        loop {
            if self.idx + 1 < self.buf.len() {
                let (l, r) = (self.buf[self.idx], self.buf[self.idx + 1]);
                self.idx += 2;
                self.played += 1;
                return Some((l * self.gain, r * self.gain));
            }
            if self.ended {
                return None;
            }
            match self.rx.try_recv() {
                Ok(Chunk::Data { epoch, start, dur, samples }) => {
                    if epoch != self.epoch {
                        continue;
                    }
                    self.buf = samples;
                    self.idx = 0;
                    self.base = start;
                    self.played = 0;
                    self.dur = dur;
                    self.started = true;
                }
                Ok(Chunk::End { epoch }) => {
                    if epoch == self.epoch {
                        self.ended = true;
                        return None;
                    }
                }
                Err(TryRecvError::Empty) => {
                    *underrun = true;
                    return None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    return None;
                }
            }
        }
    }

    fn position(&self, rate: u32) -> f64 {
        self.base + self.played as f64 / rate as f64
    }

    fn remaining(&self, rate: u32) -> Option<f64> {
        self.dur.map(|d| d - self.position(rate))
    }

    fn seek(&mut self, t: f64) {
        self.epoch += 1;
        self.buf.clear();
        self.idx = 0;
        self.base = t;
        self.played = 0;
        self.ended = false;
        let _ = self.ctrl.send(DecCmd::Seek(t));
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// RBJ cookbook filters, the same design as the iPhone equalizer: the
/// end bands are shelves with slope 1, the rest peaking at Q = √2.
fn eq_coefficients(cfg: &EqConfig, rate: f64) -> Vec<Biquad> {
    let mut out = Vec::new();
    for (i, &hz) in crate::settings::EQ_BANDS.iter().enumerate() {
        let g = cfg.gains[i] as f64;
        if g.abs() < 0.001 || hz >= rate / 2.0 {
            continue;
        }
        let a = 10f64.powf(g / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * hz / rate;
        let (cw, sw) = (w0.cos(), w0.sin());
        let (b0, b1, b2, a0, a1, a2) = if i == 0 || i == 9 {
            let alpha = sw / 2.0 * ((a + 1.0 / a) * (1.0 / 1.0 - 1.0) + 2.0).sqrt();
            let k = 2.0 * a.sqrt() * alpha;
            if i == 0 {
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + k),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - k),
                    (a + 1.0) + (a - 1.0) * cw + k,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - k,
                )
            } else {
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + k),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - k),
                    (a + 1.0) - (a - 1.0) * cw + k,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - k,
                )
            }
        } else {
            let alpha = sw / (2.0 * std::f64::consts::SQRT_2);
            (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a)
        };
        let c = Biquad {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
        };
        if [c.b0, c.b1, c.b2, c.a1, c.a2].iter().all(|x| x.is_finite()) {
            out.push(c);
        }
    }
    out
}

enum DeckCmd {
    SetCurrent(Option<DeckTrack>),
    SetNext(Option<DeckTrack>),
    Seek(f64),
    Eq(bool, f32, Vec<Biquad>),
    Transitions(f32, bool),
}

#[derive(Default)]
struct Report {
    seq: u64,
    generation: Option<u64>,
    position: f64,
    started: bool,
    underrun: bool,
    ended: Option<u64>,
    switched_to: Option<u64>,
    mono: Vec<f32>,
    mono_seq: u64,
}

struct Deck {
    rate: u32,
    cmds: Receiver<DeckCmd>,
    report: Arc<Mutex<Report>>,
    paused: Arc<AtomicBool>,
    cur: Option<DeckTrack>,
    next: Option<DeckTrack>,
    crossfade: f32,
    gapless: bool,
    fade: Option<(u64, u64)>,
    eq_on: bool,
    eq_pre: f32,
    eq: Vec<Biquad>,
    eq_z: Vec<[[f32; 2]; 2]>,
    /// Ramps 0..1 so pausing and resuming doesn't click.
    level: f32,
    second: Option<f32>,
    frames: u64,
    underrun: bool,
    ended: Option<u64>,
    switched: Option<u64>,
    ring: Vec<f32>,
    ring_i: usize,
}

impl Deck {
    fn poll(&mut self) {
        while let Ok(c) = self.cmds.try_recv() {
            match c {
                DeckCmd::SetCurrent(t) => {
                    self.cur = t;
                    self.fade = None;
                    self.ended = None;
                }
                DeckCmd::SetNext(t) => {
                    self.next = t;
                    if self.next.is_none() {
                        self.fade = None;
                    }
                }
                DeckCmd::Seek(t) => {
                    if let Some(c) = &mut self.cur {
                        c.seek(t);
                    }
                    self.fade = None;
                    self.ended = None;
                }
                DeckCmd::Eq(on, pre, c) => {
                    self.eq_on = on;
                    self.eq_pre = pre;
                    self.eq_z = vec![[[0.0; 2]; 2]; c.len()];
                    self.eq = c;
                }
                DeckCmd::Transitions(x, g) => {
                    self.crossfade = x;
                    self.gapless = g;
                }
            }
        }
    }

    fn publish(&mut self) {
        let Ok(mut r) = self.report.try_lock() else { return };
        r.seq += 1;
        r.generation = self.cur.as_ref().map(|c| c.generation);
        r.position = self.cur.as_ref().map(|c| c.position(self.rate)).unwrap_or(0.0);
        r.started = self.cur.as_ref().is_some_and(|c| c.started);
        r.underrun = self.underrun;
        if self.ended.is_some() {
            r.ended = self.ended.take();
        }
        if self.switched.is_some() {
            r.switched_to = self.switched.take();
        }
        if self.frames % 1024 == 0 {
            let n = self.ring.len();
            r.mono.clear();
            r.mono.extend_from_slice(&self.ring[self.ring_i..]);
            r.mono.extend_from_slice(&self.ring[..self.ring_i]);
            debug_assert_eq!(r.mono.len(), n);
            r.mono_seq += 1;
        }
    }

    fn switch_to_next(&mut self) -> bool {
        match self.next.take() {
            Some(n) => {
                self.switched = Some(n.generation);
                self.cur = Some(n);
                self.fade = None;
                true
            }
            None => false,
        }
    }

    fn frame(&mut self) -> (f32, f32) {
        let mut underrun = false;
        let mut out = (0.0, 0.0);
        if let Some(cur) = &mut self.cur {
            // Start a crossfade once the current track is close enough to its end.
            if self.fade.is_none() && self.crossfade > 0.0 && self.next.is_some() {
                if let Some(rem) = cur.remaining(self.rate) {
                    if rem <= self.crossfade as f64 && rem > 0.05 && cur.started {
                        self.fade = Some((0, (rem * self.rate as f64) as u64));
                    }
                }
            }
            let a = cur.pull(&mut underrun);
            let cur_ended = a.is_none() && cur.ended;
            if let Some((done, total)) = self.fade {
                let mut nu = false;
                let b = self.next.as_mut().and_then(|n| n.pull(&mut nu)).unwrap_or((0.0, 0.0));
                let t = (done as f32 / total.max(1) as f32).min(1.0);
                let (ga, gb) = ((t * std::f32::consts::FRAC_PI_2).cos(), (t * std::f32::consts::FRAC_PI_2).sin());
                let a = a.unwrap_or((0.0, 0.0));
                out = (a.0 * ga + b.0 * gb, a.1 * ga + b.1 * gb);
                self.fade = Some((done + 1, total));
                if done + 1 >= total || cur_ended {
                    self.switch_to_next();
                }
                underrun = false;
            } else if let Some(a) = a {
                out = a;
            } else if cur_ended {
                let generation = cur.generation;
                if (self.gapless || self.crossfade > 0.0) && self.switch_to_next() {
                    let mut u = false;
                    out = self.cur.as_mut().and_then(|c| c.pull(&mut u)).unwrap_or((0.0, 0.0));
                } else {
                    self.ended = Some(generation);
                    self.cur = None;
                }
            }
        }
        self.underrun = underrun;
        out
    }

    fn process(&mut self, (mut l, mut r): (f32, f32)) -> (f32, f32) {
        if self.eq_on {
            l *= self.eq_pre;
            r *= self.eq_pre;
            for (c, z) in self.eq.iter().zip(self.eq_z.iter_mut()) {
                let [zl, zr] = z;
                for (x, zz) in [(&mut l, zl), (&mut r, zr)] {
                    let y = c.b0 * *x + zz[0];
                    zz[0] = c.b1 * *x - c.a1 * y + zz[1];
                    zz[1] = c.b2 * *x - c.a2 * y;
                    *x = y;
                }
            }
        }
        (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
    }
}

impl Iterator for Deck {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(r) = self.second.take() {
            return Some(r);
        }
        if self.frames % 256 == 0 {
            self.poll();
            self.publish();
        }
        self.frames += 1;
        let paused = self.paused.load(Ordering::Relaxed);
        let target = if paused { 0.0 } else { 1.0 };
        // About 20 ms either way at 44.1 kHz.
        let step = 1.0 / 900.0;
        if self.level < target {
            self.level = (self.level + step).min(1.0);
        } else if self.level > target {
            self.level = (self.level - step).max(0.0);
        }
        if paused && self.level == 0.0 {
            self.second = Some(0.0);
            return Some(0.0);
        }
        let f = self.frame();
        let (l, r) = self.process(f);
        let (l, r) = (l * self.level, r * self.level);
        self.ring[self.ring_i] = (l + r) * 0.5;
        self.ring_i = (self.ring_i + 1) % self.ring.len();
        self.second = Some(r);
        Some(l)
    }
}

impl Source for Deck {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

// ---------------------------------------------------------------- spectrum

/// 24 log-spaced Goertzel bands, like the iPhone player's spectrum.
fn spectrum(samples: &[f32], rate: f64) -> [f32; BANDS] {
    let n = 1024.min(samples.len());
    let mut out = [0.0; BANDS];
    if n < 256 {
        return out;
    }
    let s = &samples[samples.len() - n..];
    let top = (rate * 0.45).min(16000.0);
    for (b, o) in out.iter_mut().enumerate() {
        let hz = 55.0 * (top / 55.0).powf(b as f64 / (BANDS - 1) as f64);
        let coeff = 2.0 * (2.0 * std::f64::consts::PI * hz / rate).cos();
        let (mut p, mut q) = (0.0f64, 0.0f64);
        for (i, &x) in s.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
            let cur = x as f64 * w + coeff * p - q;
            q = p;
            p = cur;
        }
        let power = (p * p + q * q - coeff * p * q).max(0.0);
        let mag = power.sqrt() / (n as f64 / 4.0);
        // Music loses energy as pitch rises, so tilt the bands up 3 dB an
        // octave or the right half of the bars would hardly move.
        let db = 20.0 * mag.max(1e-9).log10() + 3.0 * (hz / 55.0).log2();
        *o = (((db + 66.0) / 60.0).clamp(0.0, 1.0)) as f32;
    }
    out
}

// ---------------------------------------------------------------- engine thread

struct Track {
    generation: u64,
    shared: Option<Arc<Shared>>,
    info: Arc<Mutex<Info>>,
}

impl Track {
    fn cancel(&self) {
        if let Some(s) = &self.shared {
            s.cancelled.store(true, Ordering::SeqCst);
            s.cond.notify_all();
        }
    }
}

fn open(input: Input, generation: u64, rate: u32, start_at: f64, gain: f32) -> (Track, DeckTrack) {
    let info = Arc::new(Mutex::new(Info::default()));
    let (input, shared) = match input.source {
        InputSource::Stream { reader, len, reopen, cache } => {
            let sh = Arc::new(Shared {
                data: Mutex::new(Vec::with_capacity(len.unwrap_or(8 << 20).min(512 << 20) as usize)),
                cond: Condvar::new(),
                done: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                total: len,
            });
            spawn_download(reader, sh.clone(), reopen, cache);
            let placeholder: Box<dyn Read + Send> = Box::new(std::io::empty());
            (Input { source: InputSource::Stream { reader: placeholder, len, reopen: None, cache: None }, hint: input.hint }, Some(sh))
        }
        src => (Input { source: src, hint: input.hint }, None),
    };
    let (tx, rx) = bounded(CHUNKS_AHEAD);
    let (ctx, crx) = unbounded();
    spawn_decoder(input, rate, start_at, info.clone(), tx, crx, shared.clone());
    let deck = DeckTrack {
        generation,
        rx,
        ctrl: ctx,
        gain,
        buf: Vec::new(),
        idx: 0,
        epoch: 0,
        base: start_at,
        played: 0,
        dur: None,
        ended: false,
        started: false,
    };
    (Track { generation, shared, info }, deck)
}

fn run(rx: Receiver<Cmd>, status: Arc<Mutex<Status>>, spec: Arc<Mutex<[f32; BANDS]>>, volume: f32, ready: Sender<Result<(), String>>) {
    let mut sink: MixerDeviceSink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            crate::log::error(format!("no audio output: {e}"));
            let _ = ready.send(Err(format!("No audio output: {e}")));
            return;
        }
    };
    sink.log_on_drop(false);
    let rate = sink.config().sample_rate().get();
    crate::log::info(format!("audio output at {rate} Hz"));
    let player = Player::connect_new(sink.mixer());
    player.set_volume(volume);
    let (dtx, drx) = unbounded();
    let report = Arc::new(Mutex::new(Report { mono: vec![0.0; 2048], ..Default::default() }));
    let paused = Arc::new(AtomicBool::new(true));
    player.append(Deck {
        rate,
        cmds: drx,
        report: report.clone(),
        paused: paused.clone(),
        cur: None,
        next: None,
        crossfade: 0.0,
        gapless: true,
        fade: None,
        eq_on: false,
        eq_pre: 1.0,
        eq: Vec::new(),
        eq_z: Vec::new(),
        level: 0.0,
        second: None,
        frames: 0,
        underrun: false,
        ended: None,
        switched: None,
        ring: vec![0.0; 2048],
        ring_i: 0,
    });
    player.play();
    let _ = ready.send(Ok(()));

    let mut cur: Option<Track> = None;
    let mut next: Option<Track> = None;
    let mut error_sent = false;
    let mut last_seq = 0u64;
    let mut last_mono = 0u64;
    let mut smoothed = [0.0f32; BANDS];
    let mut last_spec = Instant::now();

    loop {
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(Cmd::Load { input, generation, start_at, paused: p, gain }) => {
                if let Some(t) = cur.take() {
                    t.cancel();
                }
                if let Some(t) = next.take() {
                    t.cancel();
                }
                let (t, d) = open(input, generation, rate, start_at, gain);
                cur = Some(t);
                error_sent = false;
                let _ = dtx.send(DeckCmd::SetNext(None));
                let _ = dtx.send(DeckCmd::SetCurrent(Some(d)));
                paused.store(p, Ordering::Relaxed);
                let mut st = status.lock().unwrap();
                *st = Status { generation, loading: true, paused: p, position: start_at, ..Default::default() };
            }
            Ok(Cmd::Preload { input, generation, gain }) => {
                if let Some(t) = next.take() {
                    t.cancel();
                }
                let (t, d) = open(input, generation, rate, 0.0, gain);
                next = Some(t);
                let _ = dtx.send(DeckCmd::SetNext(Some(d)));
                status.lock().unwrap().preloaded = Some(generation);
            }
            Ok(Cmd::ClearPreload) => {
                if let Some(t) = next.take() {
                    t.cancel();
                }
                let _ = dtx.send(DeckCmd::SetNext(None));
                status.lock().unwrap().preloaded = None;
            }
            Ok(Cmd::Toggle) => {
                let p = !paused.load(Ordering::Relaxed);
                paused.store(p, Ordering::Relaxed);
                status.lock().unwrap().paused = p;
            }
            Ok(Cmd::Stop) => {
                if let Some(t) = cur.take() {
                    t.cancel();
                }
                if let Some(t) = next.take() {
                    t.cancel();
                }
                let _ = dtx.send(DeckCmd::SetNext(None));
                let _ = dtx.send(DeckCmd::SetCurrent(None));
                paused.store(true, Ordering::Relaxed);
                let mut st = status.lock().unwrap();
                let g = st.generation;
                *st = Status { generation: g, paused: true, ..Default::default() };
            }
            Ok(Cmd::Seek(secs)) => {
                let _ = dtx.send(DeckCmd::Seek(secs.max(0.0)));
                status.lock().unwrap().position = secs.max(0.0);
                last_seq = u64::MAX;
            }
            Ok(Cmd::Volume(v)) => player.set_volume(v.clamp(0.0, 1.5)),
            Ok(Cmd::Eq(cfg)) => {
                let c = eq_coefficients(&cfg, rate as f64);
                let pre = 10f32.powf(cfg.preamp_db / 20.0);
                let _ = dtx.send(DeckCmd::Eq(cfg.enabled, pre, c));
            }
            Ok(Cmd::Transitions { crossfade, gapless }) => {
                let _ = dtx.send(DeckCmd::Transitions(crossfade, gapless));
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }

        // Fold the deck's report into the status.
        let (rep_seq, rep_gen, rep_pos, started, underrun, ended, switched, mono) = {
            let mut r = report.lock().unwrap();
            let mono = if r.mono_seq != last_mono {
                last_mono = r.mono_seq;
                Some(r.mono.clone())
            } else {
                None
            };
            (r.seq, r.generation, r.position, r.started, r.underrun, r.ended.take(), r.switched_to.take(), mono)
        };
        let is_paused = paused.load(Ordering::Relaxed);
        let mut st = status.lock().unwrap();
        if let Some(g) = switched {
            if next.as_ref().is_some_and(|n| n.generation == g) {
                crate::log::info("moved on to the next song with no gap");
                cur = next.take();
                error_sent = false;
                *st = Status { generation: g, playing: true, paused: is_paused, ..Default::default() };
            }
        }
        if let Some(t) = &cur {
            let info = t.info.lock().unwrap();
            st.duration = info.duration;
            st.seekable = info.seekable;
            if let (Some(e), false) = (&info.error, error_sent) {
                error_sent = true;
                st.error = Some(e.clone());
                st.ended = true;
                st.playing = false;
                st.loading = false;
            } else if info.error.is_none() {
                let live = rep_gen == Some(t.generation);
                if live && rep_seq != last_seq && last_seq != u64::MAX {
                    st.position = rep_pos;
                }
                if live {
                    last_seq = rep_seq;
                    if last_seq == u64::MAX {
                        last_seq = 0;
                    }
                }
                st.playing = info.ready;
                st.loading = !info.ready || (!is_paused && live && (!started || underrun));
                if let Some(sh) = &t.shared {
                    let have = sh.data.lock().unwrap().len() as f64;
                    st.buffered = match (sh.total, info.duration) {
                        _ if sh.done.load(Ordering::SeqCst) => info.duration.unwrap_or(f64::MAX),
                        (Some(total), Some(d)) if total > 0 => have / total as f64 * d,
                        _ => 0.0,
                    };
                } else {
                    st.buffered = info.duration.unwrap_or(f64::MAX);
                }
            }
            if ended == Some(t.generation) && info.error.is_none() {
                crate::log::info("song finished");
                st.ended = true;
                st.playing = false;
            }
        }
        st.paused = is_paused;
        st.preloaded = next.as_ref().map(|n| n.generation);
        drop(st);

        if let Some(m) = mono {
            if last_spec.elapsed() >= Duration::from_millis(30) {
                last_spec = Instant::now();
                let raw = spectrum(&m, rate as f64);
                for (s, r) in smoothed.iter_mut().zip(raw) {
                    *s = if r > *s { r } else { *s * 0.82 + r * 0.18 };
                }
                *spec.lock().unwrap() = smoothed;
            }
        } else if is_paused || cur.is_none() {
            for s in smoothed.iter_mut() {
                *s *= 0.8;
            }
            *spec.lock().unwrap() = smoothed;
        }
    }
}
