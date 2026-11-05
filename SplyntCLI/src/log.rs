//! A plain text log, so problems can be looked at after the fact.
//!
//! macOS: `~/Library/Logs/splynt/splynt.log`. Elsewhere it sits next to the
//! cache. The file rolls over to `splynt.log.1` past 2 MB. Request URLs never
//! go in it, because they carry the Subsonic token.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("SPLYNT_LOG") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        return home.join("Library").join("Logs").join("splynt").join("splynt.log");
    }
    crate::config::cache_dir().join("splynt.log")
}

pub fn init() {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        *FILE.lock().unwrap() = Some(f);
    }
    std::panic::set_hook(Box::new(|info| {
        write("PANIC", &info.to_string());
    }));
    write("INFO", &format!("splynt {} started", env!("CARGO_PKG_VERSION")));
}

fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // UTC, without pulling in a date crate.
    let (days, rem) = (secs / 86400, secs % 86400);
    let (h, m, s) = (rem / 3600, (rem / 60) % 60, rem % 60);
    let (y, mo, d) = civil(days as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn write(level: &str, msg: &str) {
    let line = format!("{} {level:<5} {}\n", stamp(), scrub(msg));
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Drops the `t=` and `s=` query values in case a URL slipped into a message.
fn scrub(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(i) = rest.find(['?', '&']) {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        if rest.starts_with("t=") || rest.starts_with("s=") {
            out.push_str(&rest[..2]);
            out.push_str("…");
            let end = rest.find(['&', ' ', '"']).unwrap_or(rest.len());
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

pub fn info(msg: impl AsRef<str>) {
    write("INFO", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    write("WARN", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    write("ERROR", msg.as_ref());
}

/// The last `n` lines, for the About page.
pub fn tail(n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}
