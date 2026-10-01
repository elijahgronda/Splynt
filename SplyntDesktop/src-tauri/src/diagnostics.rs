//! On-disk diagnostics for the desktop client.
//!
//! The webview already forwarded its warnings and errors to the host, but the
//! host wrote them to stderr — which a terminal captures under `tauri dev` and
//! which macOS sends to /dev/null for an app launched from Finder. So the app
//! was talking and nothing was listening, and any problem had to be reproduced
//! under the dev server to be readable at all.
//!
//! This is the same shape as the iOS `DiagnosticsLog`, for the same reason: the
//! person who hits the bug is not in a position to have a debugger attached at
//! the moment it happens, so the app records its own evidence to a plain
//! JSON-lines file they can send on afterwards.
//!
//! Three things it captures that stderr never could:
//!
//!   1. A timeline across launches, so a repeated failure is visible as one.
//!   2. An unclean-exit marker — a file written at launch and removed on a
//!      normal exit. Still there next launch means the previous session died
//!      without running its shutdown, which is the only way a process can
//!      report its own crash.
//!   3. Rust panics, via a hook, which otherwise vanish with the process.
//!
//! Written on a dedicated thread so logging never blocks whatever is calling it.

use serde_json::{Map, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{mpsc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// Kept small enough that rotating is cheap and large enough to span several
/// launches — a repeated failure is only diagnosable if all of its attempts
/// are still in the file.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

struct Sink {
    sender: mpsc::Sender<String>,
    log_path: PathBuf,
    marker_path: PathBuf,
}

static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();

fn sink() -> &'static Mutex<Option<Sink>> {
    SINK.get_or_init(|| Mutex::new(None))
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Seconds since the epoch plus millis, rendered as RFC3339-ish UTC without
    // pulling in a date library for one line of output.
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Howard Hinnant's civil-from-days, which is the standard way to do this
/// without a calendar dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Starts the log and reports whether the previous session ended cleanly.
pub(crate) fn start(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let log_path = dir.join("splice.log");
    let marker_path = dir.join("session.marker");
    rotate_if_large(&log_path);

    let previous_unclean = marker_path.exists();
    let previous_seen = fs::metadata(&marker_path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs());
    let _ = fs::write(&marker_path, b"active");

    let (sender, receiver) = mpsc::channel::<String>();
    let writer_path = log_path.clone();
    std::thread::Builder::new()
        .name("splice-diagnostics".into())
        .spawn(move || {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&writer_path)
                .ok();
            while let Ok(line) = receiver.recv() {
                if let Some(handle) = file.as_mut() {
                    let _ = handle.write_all(line.as_bytes());
                    let _ = handle.flush();
                }
            }
        })
        .ok();

    if let Ok(mut guard) = sink().lock() {
        *guard = Some(Sink {
            sender,
            log_path,
            marker_path,
        });
    }

    if previous_unclean {
        let mut detail = Map::new();
        if let Some(seen) = previous_seen {
            detail.insert("previousSessionLastSeen".into(), Value::from(seen));
        }
        // The headline entry: several of these in a row with no clean exit
        // between them is a crash loop, recorded without a debugger attached.
        log_detail("previous_session_ended_abnormally", detail);
    }

    let mut detail = Map::new();
    detail.insert(
        "version".into(),
        Value::from(app.package_info().version.to_string()),
    );
    detail.insert("os".into(), Value::from(std::env::consts::OS));
    detail.insert("arch".into(), Value::from(std::env::consts::ARCH));
    // The id other devices see this one as. Two exported logs could not be
    // proved to be a pair before: neither said who wrote it, and no Connect
    // line named the device on the other end.
    detail.insert(
        "device".into(),
        Value::from(crate::connect::persistent_device_id()),
    );
    log_detail("launch", detail);

    install_panic_hook();
}

/// Removed on a normal shutdown, so its presence next launch means the previous
/// session died without getting here.
pub(crate) fn mark_clean_exit() {
    log("clean_exit");
    if let Ok(guard) = sink().lock() {
        if let Some(sink) = guard.as_ref() {
            let _ = fs::remove_file(&sink.marker_path);
        }
    }
}

pub(crate) fn log(event: &str) {
    log_detail(event, Map::new());
}

pub(crate) fn log_detail(event: &str, detail: Map<String, Value>) {
    let mut entry = Map::new();
    entry.insert("t".into(), Value::from(timestamp()));
    entry.insert("event".into(), Value::from(event));
    for (key, value) in detail {
        entry.insert(key, value);
    }
    let Ok(mut line) = serde_json::to_string(&Value::Object(entry)) else {
        return;
    };
    line.push('\n');
    // Also to stderr, which is where it shows up under `tauri dev`.
    eprint!("{line}");
    if let Ok(guard) = sink().lock() {
        if let Some(sink) = guard.as_ref() {
            let _ = sink.sender.send(line);
        }
    }
}

pub(crate) fn log_path() -> Option<PathBuf> {
    sink()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|sink| sink.log_path.clone()))
}

/// A panic in the host otherwise disappears with the process.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut detail = Map::new();
        detail.insert("payload".into(), Value::from(info.to_string()));
        if let Some(location) = info.location() {
            detail.insert(
                "location".into(),
                Value::from(format!("{}:{}", location.file(), location.line())),
            );
        }
        log_detail("host_panic", detail);
        previous(info);
    }));
}

/// Keeps the newest half rather than deleting the file, so a rotation never
/// throws away the run that is about to be investigated.
fn rotate_if_large(path: &PathBuf) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() <= MAX_BYTES {
        return;
    }
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::End(-(MAX_BYTES as i64) / 2)).is_err() {
        return;
    }
    let mut tail = String::new();
    if file.read_to_string(&mut tail).is_err() {
        return;
    }
    // Drop the partial first line left by seeking into the middle of one.
    let tail = tail.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
    let _ = fs::write(path, tail.as_bytes());
}
