use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

const SERVICE_TYPE: &str = "_spliceconnect._tcp.local.";
const MAX_CONNECTIONS: usize = 64;
const MAX_PENDING_COMMANDS: usize = 256;
const MAX_HANDOFF_TRACKS: usize = 1_000;
/// Wire contract: an unterminated buffer larger than this is discarded.
const MAX_BUFFERED_FRAME: usize = 262_144;
/// Eight clock samples over the 24 s the heartbeat takes to collect them.
const MAX_CLOCK_SAMPLES: usize = 8;
/// A sample older than this describes a clock the device may no longer have,
/// and a probe unanswered this long is never going to be.
const CLOCK_RETENTION: Duration = Duration::from_secs(30);

/// Wire field names come from `SplyntConnect.swift`, which `docs/connect/WIRE-V1.md`
/// names as the contract this file has to match byte-for-byte. Swift synthesises
/// its coding keys from property names, so an id key is `trackID` and a plain
/// `rename_all = "camelCase"` writes `trackId` instead. That one letter meant
/// every frame carrying a track id was dropped between a desktop and a phone,
/// and a state frame still decoded but always read as idle. The renames below
/// are the whole fix; do not remove one because it looks redundant next to the
/// container's camelCase rule.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectPlayback {
    #[serde(rename = "trackID")]
    pub(crate) track_id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    #[serde(rename = "coverArtID")]
    pub(crate) cover_art_id: Option<String>,
    pub(crate) is_playing: bool,
    pub(crate) position: f64,
    pub(crate) duration: f64,
}

/// What a device is currently committed to, published on every state frame.
/// See `SplyntConnectCommitment` in `SplyntConnect.swift`: this is the field
/// that makes "an output belongs to one session at a time" checkable.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectCommitment {
    #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(rename = "leaderID", skip_serializing_if = "Option::is_none")]
    pub(crate) leader_id: Option<String>,
    pub(crate) revision: i64,
    #[serde(rename = "controllingPeerID", skip_serializing_if = "Option::is_none")]
    pub(crate) controlling_peer_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectPeer {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) platform: String,
    pub(crate) playback: ConnectPlayback,
    pub(crate) updated_at: f64,
    /// Optional so a v1 peer that never sends one still decodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) commitment: Option<ConnectCommitment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectHandoff {
    #[serde(rename = "trackIDs")]
    pub(crate) track_ids: Vec<String>,
    #[serde(rename = "currentTrackID")]
    pub(crate) current_track_id: String,
    pub(crate) position: f64,
    pub(crate) is_playing: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectGroup {
    pub(crate) id: String,
    #[serde(rename = "leaderID")]
    pub(crate) leader_id: String,
    #[serde(rename = "trackID")]
    pub(crate) track_id: String,
    pub(crate) position: f64,
    pub(crate) is_playing: bool,
    pub(crate) sent_at: f64,
    /// Monotonic within a session. Absent from a v1 peer's frames, read as 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<i64>,
}

/// A follower's answer to a group join. v1 had none, which is why a leader
/// could report a member it had never reached.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectGroupReply {
    #[serde(rename = "sessionID")]
    pub(crate) session_id: String,
    #[serde(rename = "deviceID")]
    pub(crate) device_id: String,
    pub(crate) revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectGroupJoin {
    pub(crate) group: ConnectGroup,
    pub(crate) handoff: ConnectHandoff,
}

/// One round of the four-timestamp exchange that measures how far a peer's
/// clock sits from this one. Unix milliseconds, like `ConnectGroup::sent_at`.
/// `t2`/`t3` are absent on the request and present on the reply.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectTimeProbe {
    pub(crate) id: String,
    pub(crate) t1: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) t2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) t3: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectCommand {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) handoff: Option<ConnectHandoff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) group: Option<ConnectGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) group_join: Option<ConnectGroupJoin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) group_reply: Option<ConnectGroupReply>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) time: Option<ConnectTimeProbe>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectSnapshot {
    pub(crate) is_available: bool,
    pub(crate) local_device_id: Option<String>,
    pub(crate) peers: Vec<ConnectPeer>,
    pub(crate) commands: Vec<ConnectCommand>,
    /// Milliseconds to add to this device's clock to read each peer's, keyed
    /// by peer id. Absent until that peer answers a probe.
    pub(crate) clock_offsets: HashMap<String, f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireMessage {
    kind: String,
    authentication: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer: Option<ConnectPeer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<ConnectCommand>,
}

struct PeerEntry {
    peer: ConnectPeer,
    received_at: Instant,
}

#[derive(Clone, Copy)]
struct ClockSample {
    offset_ms: f64,
    round_trip_ms: f64,
    measured_at: Instant,
}

struct PendingProbe {
    peer_id: String,
    t1: f64,
    sent_at: Instant,
}

struct DiscoveredService {
    socket: SocketAddr,
    last_attempt: Instant,
}

struct Runtime {
    alive: AtomicBool,
    authentication: String,
    local_peer: Mutex<ConnectPeer>,
    peers: Mutex<HashMap<String, PeerEntry>>,
    commands: Mutex<Vec<ConnectCommand>>,
    routes: Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>,
    service_routes: Mutex<HashMap<String, String>>,
    discovered: Mutex<HashMap<String, DiscoveredService>>,
    connections: Mutex<Vec<Arc<Mutex<TcpStream>>>>,
    clock_samples: Mutex<HashMap<String, Vec<ClockSample>>>,
    pending_probes: Mutex<HashMap<String, PendingProbe>>,
    /// Frames that arrived and could not be read, and when that was last
    /// reported. Both exist so the count survives the throttle: the number of
    /// dropped frames is the diagnostic, and the sample is only an example.
    undecodable_frames: AtomicU64,
    last_undecodable_report: Mutex<Option<Instant>>,
    mdns: ServiceDaemon,
}

#[derive(Default)]
pub(crate) struct SplyntConnectState(Mutex<Option<Arc<Runtime>>>);

impl SplyntConnectState {
    pub(crate) fn configure(
        &self,
        server: &str,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        self.stop();
        let authentication = fingerprint(server, username, password);
        let device_id = persistent_device_id();
        let device_name = hostname::get()
            .ok()
            .and_then(|name| name.into_string().ok())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Splynt Desktop".to_string());
        let platform = if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Linux"
        };
        let listener = TcpListener::bind("0.0.0.0:0")
            .map_err(|error| format!("Splynt Connect could not open a local listener: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Splynt Connect could not configure its listener: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Splynt Connect could not read its local port: {error}"))?
            .port();
        let local_ip = local_ip_address::local_ip().map_err(|error| {
            format!("Splynt Connect could not find a local network address: {error}")
        })?;
        let mdns = ServiceDaemon::new()
            .map_err(|error| format!("Splynt Connect could not start discovery: {error}"))?;
        let _ = mdns.set_service_name_len_max(32);
        let suffix: String = device_id
            .chars()
            .filter(|character| *character != '-')
            .take(8)
            .collect();
        let instance_name = format!("Splynt-{suffix}");
        let host_name = format!("splice-{suffix}.local.");
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &host_name,
            local_ip,
            port,
            None::<HashMap<String, String>>,
        )
        .map_err(|error| format!("Splynt Connect could not describe this device: {error}"))?;
        mdns.register(service)
            .map_err(|error| format!("Splynt Connect could not advertise this device: {error}"))?;

        let runtime = Arc::new(Runtime {
            alive: AtomicBool::new(true),
            authentication,
            local_peer: Mutex::new(ConnectPeer {
                id: device_id,
                name: device_name,
                platform: platform.to_string(),
                playback: ConnectPlayback::default(),
                updated_at: apple_reference_time(),
                commitment: None,
            }),
            peers: Mutex::new(HashMap::new()),
            commands: Mutex::new(Vec::new()),
            routes: Mutex::new(HashMap::new()),
            service_routes: Mutex::new(HashMap::new()),
            discovered: Mutex::new(HashMap::new()),
            connections: Mutex::new(Vec::new()),
            clock_samples: Mutex::new(HashMap::new()),
            pending_probes: Mutex::new(HashMap::new()),
            undecodable_frames: AtomicU64::new(0),
            last_undecodable_report: Mutex::new(None),
            mdns,
        });
        *self
            .0
            .lock()
            .map_err(|_| "Splynt Connect state is unavailable.".to_string())? =
            Some(runtime.clone());
        start_listener(runtime.clone(), listener);
        start_browser(runtime.clone(), instance_name)?;
        start_heartbeat(runtime);
        Ok(())
    }

    pub(crate) fn stop(&self) {
        let runtime = self.0.lock().ok().and_then(|mut value| value.take());
        if let Some(runtime) = runtime {
            runtime.alive.store(false, Ordering::Relaxed);
            let _ = runtime.mdns.shutdown();
        }
    }

    fn runtime(&self) -> Option<Arc<Runtime>> {
        self.0.lock().ok().and_then(|value| value.clone())
    }
}

#[tauri::command]
pub(crate) fn publish_connect_playback(
    playback: ConnectPlayback,
    commitment: Option<ConnectCommitment>,
    state: tauri::State<'_, SplyntConnectState>,
) -> Result<(), String> {
    let Some(runtime) = state.runtime() else {
        return Ok(());
    };
    let mut peer = runtime
        .local_peer
        .lock()
        .map_err(|_| "Splynt Connect playback is unavailable.".to_string())?;
    peer.playback = playback;
    peer.commitment = commitment;
    peer.updated_at = apple_reference_time();
    drop(peer);
    broadcast_state(&runtime);
    Ok(())
}

#[tauri::command]
pub(crate) fn connect_snapshot(state: tauri::State<'_, SplyntConnectState>) -> ConnectSnapshot {
    let Some(runtime) = state.runtime() else {
        return ConnectSnapshot {
            is_available: false,
            local_device_id: None,
            peers: Vec::new(),
            commands: Vec::new(),
            clock_offsets: HashMap::new(),
        };
    };
    let cutoff = Instant::now() - Duration::from_secs(12);
    let mut peers = runtime
        .peers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    peers.retain(|_, entry| entry.received_at >= cutoff);
    let mut values: Vec<_> = peers.values().map(|entry| entry.peer.clone()).collect();
    values.sort_by(|left, right| {
        right
            .playback
            .is_playing
            .cmp(&left.playback.is_playing)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let commands = runtime
        .commands
        .lock()
        .map(|mut commands| commands.drain(..).collect())
        .unwrap_or_default();
    let local_device_id = runtime.local_peer.lock().ok().map(|peer| peer.id.clone());
    ConnectSnapshot {
        is_available: runtime.alive.load(Ordering::Relaxed),
        local_device_id,
        peers: values,
        commands,
        clock_offsets: best_clock_offsets(&runtime),
    }
}

#[tauri::command]
pub(crate) fn send_connect_command(
    peer_id: String,
    command: ConnectCommand,
    state: tauri::State<'_, SplyntConnectState>,
) -> Result<(), String> {
    let runtime = state
        .runtime()
        .ok_or_else(|| "Splynt Connect is not available.".to_string())?;
    let route = runtime
        .routes
        .lock()
        .map_err(|_| "Splynt Connect routes are unavailable.".to_string())?
        .get(&peer_id)
        .cloned()
        .ok_or_else(|| "That device is no longer connected.".to_string())?;
    send_wire(
        &route,
        &WireMessage {
            kind: "command".to_string(),
            authentication: runtime.authentication.clone(),
            peer: None,
            command: Some(command),
        },
    )
}

fn start_listener(runtime: Arc<Runtime>, listener: TcpListener) {
    thread::spawn(move || {
        while runtime.alive.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => adopt_stream(runtime.clone(), stream, None),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(160))
                }
                Err(_) => thread::sleep(Duration::from_millis(500)),
            }
        }
    });
}

fn start_browser(runtime: Arc<Runtime>, local_instance: String) -> Result<(), String> {
    let receiver = runtime
        .mdns
        .browse(SERVICE_TYPE)
        .map_err(|error| format!("Splynt Connect could not browse for devices: {error}"))?;
    thread::spawn(move || {
        while runtime.alive.load(Ordering::Relaxed) {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    let fullname = service.get_fullname().to_string();
                    if fullname.starts_with(&format!("{local_instance}.")) {
                        continue;
                    }
                    let Some(address) = service
                        .get_addresses()
                        .iter()
                        .find(|address| address.is_ipv4())
                        .or_else(|| service.get_addresses().iter().next())
                    else {
                        continue;
                    };
                    let socket = SocketAddr::new(address.to_ip_addr(), service.get_port());
                    if let Ok(mut discovered) = runtime.discovered.lock() {
                        discovered.insert(
                            fullname,
                            DiscoveredService {
                                socket,
                                last_attempt: Instant::now() - Duration::from_secs(10),
                            },
                        );
                    }
                }
                Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                    if let Ok(mut discovered) = runtime.discovered.lock() {
                        discovered.remove(&fullname);
                    }
                    if let Ok(mut routes) = runtime.service_routes.lock() {
                        routes.remove(&fullname);
                    }
                }
                _ => {}
            }

            let connected = runtime
                .service_routes
                .lock()
                .map(|routes| routes.clone())
                .unwrap_or_default();
            let live_peers = runtime
                .peers
                .lock()
                .map(|peers| peers.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let mut attempts = Vec::new();
            if let Ok(mut discovered) = runtime.discovered.lock() {
                for (fullname, service) in discovered.iter_mut() {
                    let is_connected = connected
                        .get(fullname)
                        .is_some_and(|peer_id| live_peers.contains(peer_id));
                    if !is_connected && service.last_attempt.elapsed() >= Duration::from_secs(3) {
                        service.last_attempt = Instant::now();
                        attempts.push((fullname.clone(), service.socket));
                    }
                }
            }
            for (fullname, socket) in attempts {
                if let Ok(stream) = TcpStream::connect_timeout(&socket, Duration::from_secs(2)) {
                    adopt_stream(runtime.clone(), stream, Some(fullname));
                }
            }
        }
    });
    Ok(())
}

fn start_heartbeat(runtime: Arc<Runtime>) {
    thread::spawn(move || {
        while runtime.alive.load(Ordering::Relaxed) {
            broadcast_state(&runtime);
            probe_clocks(&runtime);
            thread::sleep(Duration::from_secs(3));
        }
    });
}

/// One clock probe per peer per heartbeat.
///
/// Two ~150-byte frames every three seconds is nothing beside the state
/// broadcast already on the wire, and probing continuously rather than only
/// during a group session means a session that starts already has an offset
/// instead of spending its first seconds measuring one.
fn probe_clocks(runtime: &Arc<Runtime>) {
    let targets: Vec<(String, Arc<Mutex<TcpStream>>)> = match runtime.routes.lock() {
        Ok(routes) => routes
            .iter()
            .map(|(peer_id, writer)| (peer_id.clone(), writer.clone()))
            .collect(),
        Err(_) => return,
    };
    for (peer_id, writer) in targets {
        let id = format!("{}-{}", peer_id, unix_millis() as u64);
        let t1 = unix_millis();
        if let Ok(mut pending) = runtime.pending_probes.lock() {
            pending.retain(|_, probe| probe.sent_at.elapsed() < CLOCK_RETENTION);
            pending.insert(
                id.clone(),
                PendingProbe {
                    peer_id: peer_id.clone(),
                    t1,
                    sent_at: Instant::now(),
                },
            );
        }
        let _ = send_wire(
            &writer,
            &WireMessage {
                kind: "command".to_string(),
                authentication: runtime.authentication.clone(),
                peer: None,
                command: Some(ConnectCommand {
                    name: "timePing".to_string(),
                    value: None,
                    handoff: None,
                    group: None,
                    group_join: None,
                    group_reply: None,
                    time: Some(ConnectTimeProbe {
                        id,
                        t1,
                        t2: None,
                        t3: None,
                    }),
                }),
            },
        );
    }
}

/// NTP's estimator, with `t4` read here as the reply lands.
///
/// The two legs cancel, so whatever the peer spent between `t2` and `t3` drops
/// out and a busy device still answers accurately. What does not cancel is an
/// asymmetric path, and the residual is bounded by half the round trip.
pub(crate) fn clock_estimate(t1: f64, t2: f64, t3: f64, t4: f64) -> Option<(f64, f64)> {
    let offset = ((t2 - t1) + (t3 - t4)) / 2.0;
    let round_trip = (t4 - t1) - (t3 - t2);
    if !offset.is_finite() || !round_trip.is_finite() || round_trip < 0.0 {
        return None;
    }
    Some((offset, round_trip))
}

fn record_clock_sample(runtime: &Arc<Runtime>, probe: &ConnectTimeProbe, arrived_at: f64) {
    let (Some(t2), Some(t3)) = (probe.t2, probe.t3) else {
        return;
    };
    // A reply is only worth anything against a request this device actually
    // sent. The id is a locally generated value, so nothing else can claim it.
    let Some(pending) = runtime
        .pending_probes
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(&probe.id))
    else {
        return;
    };
    let Some((offset_ms, round_trip_ms)) = clock_estimate(pending.t1, t2, t3, arrived_at) else {
        return;
    };
    if let Ok(mut samples) = runtime.clock_samples.lock() {
        let entry = samples.entry(pending.peer_id).or_default();
        entry.push(ClockSample {
            offset_ms,
            round_trip_ms,
            measured_at: Instant::now(),
        });
        if entry.len() > MAX_CLOCK_SAMPLES {
            let overflow = entry.len() - MAX_CLOCK_SAMPLES;
            entry.drain(..overflow);
        }
    }
}

/// The sample to believe out of a recent window.
///
/// Queueing delay only ever adds to a sample's error, so the lowest round trip
/// is the truest reading. Averaging would fold every slow sample back in.
fn best_clock_offsets(runtime: &Arc<Runtime>) -> HashMap<String, f64> {
    let Ok(mut samples) = runtime.clock_samples.lock() else {
        return HashMap::new();
    };
    for entry in samples.values_mut() {
        entry.retain(|sample| sample.measured_at.elapsed() < CLOCK_RETENTION);
    }
    samples.retain(|_, entry| !entry.is_empty());
    samples
        .iter()
        .filter_map(|(peer_id, entry)| {
            entry
                .iter()
                .min_by(|left, right| {
                    left.round_trip_ms
                        .partial_cmp(&right.round_trip_ms)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|best| (peer_id.clone(), best.offset_ms))
        })
        .collect()
}

fn adopt_stream(runtime: Arc<Runtime>, stream: TcpStream, source: Option<String>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    // Without this a peer that stops reading — a laptop that sleeps, a device
    // that drops off Wi-Fi — fills its socket buffer and blocks `write_all`
    // indefinitely. Frames are a few hundred bytes, so five seconds of no
    // progress means the peer is gone, not slow.
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(writer_stream) = stream.try_clone() else {
        return;
    };
    let writer = Arc::new(Mutex::new(writer_stream));
    if let Ok(mut connections) = runtime.connections.lock() {
        if connections.len() >= MAX_CONNECTIONS {
            return;
        }
        connections.push(writer.clone());
    }
    send_state(&runtime, &writer);
    thread::spawn(move || read_connection(runtime, stream, writer, source));
}

fn read_connection(
    runtime: Arc<Runtime>,
    mut stream: TcpStream,
    writer: Arc<Mutex<TcpStream>>,
    source: Option<String>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 65_536];
    while runtime.alive.load(Ordering::Relaxed) {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                for frame in drain_frames(&mut buffer) {
                    // Stamped before the parse, so a clock probe measures the
                    // network rather than this process's JSON decoder.
                    let arrived_at = unix_millis();
                    match serde_json::from_slice::<WireMessage>(&frame) {
                        Ok(message) => {
                            handle_wire(&runtime, &writer, source.as_deref(), message, arrived_at)
                        }
                        // A frame that will not parse used to vanish here. That
                        // silence is how a wire incompatibility survived from
                        // this file's first commit: an unreadable invitation
                        // and a peer that never answered wrote the same log,
                        // which is to say none at all.
                        Err(_) => note_undecodable_frame(&runtime, &frame),
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
    }
    if let Ok(mut connections) = runtime.connections.lock() {
        connections.retain(|candidate| !Arc::ptr_eq(candidate, &writer));
    }
    if let Ok(mut routes) = runtime.routes.lock() {
        routes.retain(|_, candidate| !Arc::ptr_eq(candidate, &writer));
    }
    if let Some(source) = source {
        if let Ok(mut routes) = runtime.service_routes.lock() {
            routes.remove(&source);
        }
    }
}

/// Splits a receive buffer into complete newline-terminated frames, leaving any
/// partial tail in place for the next read.
///
/// A peer's frames arrive fragmented, several to a chunk, or both, so this is
/// the part of the transport most worth testing and the part hardest to
/// exercise by hand. An unterminated buffer past the contract's limit is
/// discarded rather than grown without bound.
fn drain_frames(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
        let mut frame: Vec<u8> = buffer.drain(..=newline).collect();
        frame.pop();
        frames.push(frame);
    }
    if buffer.len() > MAX_BUFFERED_FRAME {
        buffer.clear();
    }
    frames
}

fn handle_wire(
    runtime: &Arc<Runtime>,
    writer: &Arc<Mutex<TcpStream>>,
    source: Option<&str>,
    message: WireMessage,
    arrived_at: f64,
) {
    if message.authentication != runtime.authentication {
        return;
    }
    match message.kind.as_str() {
        "state" => {
            let Some(mut peer) = message.peer else { return };
            let local_id = runtime.local_peer.lock().ok().map(|local| local.id.clone());
            if local_id.as_deref() == Some(peer.id.as_str()) {
                if let Ok(stream) = writer.lock() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                return;
            }
            if !valid_peer(&peer) {
                return;
            }
            peer.updated_at = apple_reference_time();
            if let Ok(mut peers) = runtime.peers.lock() {
                peers.insert(
                    peer.id.clone(),
                    PeerEntry {
                        peer: peer.clone(),
                        received_at: Instant::now(),
                    },
                );
            }
            if let Ok(mut routes) = runtime.routes.lock() {
                routes.insert(peer.id.clone(), writer.clone());
            }
            if let Some(source) = source {
                if let Ok(mut routes) = runtime.service_routes.lock() {
                    routes.insert(source.to_string(), peer.id);
                }
            }
        }
        "command" => {
            if let Some(command) = message.command {
                if !valid_command(&command) {
                    return;
                }
                // Clock probes are answered by the transport and never reach
                // the webview: the reply must not wait on a poll tick, and it
                // carries no state for the UI to apply. Neither ends with the
                // state broadcast every other command does — a probe changes
                // nothing, and at one per peer per heartbeat it would double
                // the state traffic on the LAN.
                match command.name.as_str() {
                    "timePing" => {
                        let Some(probe) = command.time else { return };
                        let reply = ConnectTimeProbe {
                            id: probe.id,
                            t1: probe.t1,
                            t2: Some(arrived_at),
                            t3: Some(unix_millis()),
                        };
                        let _ = send_wire(
                            writer,
                            &WireMessage {
                                kind: "command".to_string(),
                                authentication: runtime.authentication.clone(),
                                peer: None,
                                command: Some(ConnectCommand {
                                    name: "timePong".to_string(),
                                    value: None,
                                    handoff: None,
                                    group: None,
                                    group_join: None,
                                    group_reply: None,
                                    time: Some(reply),
                                }),
                            },
                        );
                        return;
                    }
                    "timePong" => {
                        if let Some(probe) = command.time {
                            record_clock_sample(runtime, &probe, arrived_at);
                        }
                        return;
                    }
                    _ => {}
                }
                if let Ok(mut commands) = runtime.commands.lock() {
                    if commands.len() >= MAX_PENDING_COMMANDS {
                        let overflow = commands.len() - MAX_PENDING_COMMANDS + 1;
                        commands.drain(..overflow);
                    }
                    commands.push(command)
                }
                // The buffer stays the delivery mechanism, so a command that
                // lands before the webview is listening is still applied. This
                // only removes the wait for the next poll tick, which cost
                // every remote action up to a second before it was even seen.
                announce_pending_commands();
                broadcast_state(runtime);
            }
        }
        _ => {}
    }
}

/// Reports a frame that arrived and could not be read.
///
/// Throttled to one line per 10 s with a running count, because the failure
/// this exists to catch is systematic: when two builds disagree about the
/// wire, every frame fails, at the heartbeat rate, forever.
///
/// The sample is truncated and has the fingerprint stripped. That value is a
/// replayable bearer secret on this LAN and `docs/connect/WIRE-V1.md` requires
/// it stay out of diagnostics, which matters most here, where the whole point
/// of the sample is that a person reads it.
fn note_undecodable_frame(runtime: &Arc<Runtime>, frame: &[u8]) {
    let total = runtime.undecodable_frames.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let Ok(mut last) = runtime.last_undecodable_report.lock() else {
            return;
        };
        if last.is_some_and(|at: Instant| at.elapsed() < Duration::from_secs(10)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let text = String::from_utf8_lossy(&frame[..frame.len().min(400)]);
    let redacted = redact_fingerprint(&text);
    let mut detail = serde_json::Map::new();
    detail.insert("total".into(), Value::from(total));
    detail.insert("bytes".into(), Value::from(frame.len()));
    detail.insert(
        "sample".into(),
        Value::from(redacted.chars().take(200).collect::<String>()),
    );
    crate::diagnostics::log_detail("connect_frame_undecodable", detail);
}

/// Replaces the value of an `authentication` field wherever it appears. Written
/// by hand rather than with a regex crate: the input is one short line and this
/// is the only place that needs it.
fn redact_fingerprint(text: &str) -> String {
    const KEY: &str = "\"authentication\"";
    let Some(key_at) = text.find(KEY) else {
        return text.to_string();
    };
    let after_key = &text[key_at + KEY.len()..];
    let Some(open_rel) = after_key.find('"') else {
        return text.to_string();
    };
    let value_start = key_at + KEY.len() + open_rel + 1;
    match text[value_start..].find('"') {
        Some(len) => format!(
            "{}\"<redacted>{}",
            &text[..value_start],
            &text[value_start + len..]
        ),
        None => format!("{}\"<redacted>\"", &text[..value_start]),
    }
}

fn valid_peer(peer: &ConnectPeer) -> bool {
    !peer.id.is_empty()
        && peer.id.len() <= 128
        && !peer.name.is_empty()
        && peer.name.len() <= 256
        && peer.platform.len() <= 64
        && peer.playback.position.is_finite()
        && peer.playback.position >= 0.0
        && peer.playback.duration.is_finite()
        && peer.playback.duration >= 0.0
}

fn valid_command(command: &ConnectCommand) -> bool {
    let shape_valid = match command.name.as_str() {
        "play" | "pause" | "toggle" | "previous" | "next" => true,
        "seek" => command
            .value
            .is_some_and(|value| value.is_finite() && value >= 0.0),
        "handoff" => command.handoff.as_ref().is_some_and(valid_handoff),
        "groupJoin" => command
            .group_join
            .as_ref()
            .is_some_and(|join| valid_group(&join.group) && valid_handoff(&join.handoff)),
        "groupSync" | "groupLeave" => command.group.as_ref().is_some_and(valid_group),
        "groupAccept" | "groupDecline" => {
            command.group_reply.as_ref().is_some_and(valid_group_reply)
        }
        "timePing" | "timePong" => command.time.as_ref().is_some_and(valid_time_probe),
        _ => false,
    };
    shape_valid
}

fn valid_handoff(handoff: &ConnectHandoff) -> bool {
    !handoff.current_track_id.is_empty()
        && handoff.current_track_id.len() <= 1_024
        && !handoff.track_ids.is_empty()
        && handoff.track_ids.len() <= MAX_HANDOFF_TRACKS
        && handoff
            .track_ids
            .iter()
            .all(|id| !id.is_empty() && id.len() <= 1_024)
        && handoff.position.is_finite()
        && handoff.position >= 0.0
}

fn valid_group_reply(reply: &ConnectGroupReply) -> bool {
    !reply.session_id.is_empty()
        && reply.session_id.len() <= 128
        && !reply.device_id.is_empty()
        && reply.device_id.len() <= 128
        && reply
            .reason
            .as_ref()
            .is_none_or(|reason| reason.len() <= 256)
}

fn valid_time_probe(probe: &ConnectTimeProbe) -> bool {
    !probe.id.is_empty()
        && probe.id.len() <= 128
        && probe.t1.is_finite()
        && probe.t2.is_none_or(f64::is_finite)
        && probe.t3.is_none_or(f64::is_finite)
}

fn valid_group(group: &ConnectGroup) -> bool {
    !group.id.is_empty()
        && group.id.len() <= 128
        && !group.leader_id.is_empty()
        && group.leader_id.len() <= 128
        && !group.track_id.is_empty()
        && group.track_id.len() <= 1_024
        && group.position.is_finite()
        && group.position >= 0.0
        && group.sent_at.is_finite()
        && group.sent_at >= 0.0
}

/// Broadcasts on a snapshot of the connection list rather than under its lock.
///
/// Holding the lock across a blocking `write_all` let one unresponsive peer
/// stall the heartbeat thread, every reader thread's post-command broadcast,
/// and outgoing commands, all at once. The write timeout bounds a single slow
/// socket; taking the snapshot keeps that cost off every other peer.
fn broadcast_state(runtime: &Arc<Runtime>) {
    let targets: Vec<Arc<Mutex<TcpStream>>> = match runtime.connections.lock() {
        Ok(connections) => connections.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let mut dead: Vec<Arc<Mutex<TcpStream>>> = Vec::new();
    for connection in &targets {
        if !send_state(runtime, connection) {
            dead.push(connection.clone());
        }
    }
    if dead.is_empty() {
        return;
    }
    // A partial write leaves that peer's framing broken, so a failed send
    // retires the socket instead of being retried. The browser thread redials
    // it once the service is seen again.
    let is_dead =
        |candidate: &Arc<Mutex<TcpStream>>| dead.iter().any(|gone| Arc::ptr_eq(candidate, gone));
    if let Ok(mut connections) = runtime.connections.lock() {
        connections.retain(|candidate| !is_dead(candidate));
    }
    if let Ok(mut routes) = runtime.routes.lock() {
        routes.retain(|_, candidate| !is_dead(candidate));
    }
}

fn send_state(runtime: &Arc<Runtime>, connection: &Arc<Mutex<TcpStream>>) -> bool {
    let peer = match runtime.local_peer.lock() {
        Ok(mut peer) => {
            peer.updated_at = apple_reference_time();
            peer.clone()
        }
        Err(_) => return false,
    };
    send_wire(
        connection,
        &WireMessage {
            kind: "state".to_string(),
            authentication: runtime.authentication.clone(),
            peer: Some(peer),
            command: None,
        },
    )
    .is_ok()
}

fn send_wire(connection: &Arc<Mutex<TcpStream>>, message: &WireMessage) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(message)
        .map_err(|_| "Splynt Connect could not encode a message.".to_string())?;
    bytes.push(b'\n');
    connection
        .lock()
        .map_err(|_| "Splynt Connect lost the device connection.".to_string())?
        .write_all(&bytes)
        .map_err(|error| format!("Splynt Connect could not send to that device: {error}"))
}

/// Read from the Keychain at most once per process.
///
/// This is a second Keychain item alongside the saved credentials, and both
/// dialogs name the same service, so a user cannot tell them apart — they only
/// see Splynt asking again. `configure` runs on connect, on session restore and
/// on every profile switch, so without this the device id was re-read each
/// time. The value is a stable per-install identifier, so one read is all it
/// can ever need.
static DEVICE_ID: OnceLock<String> = OnceLock::new();
static DEVICE_ID_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set once at startup, where the app data directory is known.
pub(crate) fn set_device_id_path(path: PathBuf) {
    let _ = DEVICE_ID_PATH.set(path);
}

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Set once at startup so the reader threads can wake the webview the moment a
/// command frame arrives rather than leaving it for the next snapshot poll.
pub(crate) fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

fn announce_pending_commands() {
    if let Some(handle) = APP_HANDLE.get() {
        let _ = handle.emit("connect-commands-pending", ());
    }
}

pub(crate) fn persistent_device_id() -> String {
    DEVICE_ID.get_or_init(load_persistent_device_id).clone()
}

/// A LAN identifier, not a secret.
///
/// Keeping it in the Keychain bought no protection and cost a second macOS
/// permission dialog, so it lives beside the rest of the app's data instead.
/// Older installs simply receive a new Connect identity the first time this
/// file-backed version runs; avoiding an unexplained password prompt is more
/// important than preserving an internal peer identifier.
fn load_persistent_device_id() -> String {
    if let Some(path) = DEVICE_ID_PATH.get() {
        if let Ok(saved) = std::fs::read_to_string(path) {
            let saved = saved.trim();
            if !saved.is_empty() {
                return saved.to_string();
            }
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(path) = DEVICE_ID_PATH.get() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &id);
    }
    id
}

/// Unix milliseconds. The clock probe and `ConnectGroup::sent_at` both use
/// this, unlike `ConnectPeer::updated_at`, which is on Apple's epoch.
fn unix_millis() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1_000.0
}

fn apple_reference_time() -> f64 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    unix - 978_307_200.0
}

/// The port as written in the profile, or none if the listener omitted it.
///
/// Swift reaches this through `URLComponents`, which reports exactly what was
/// typed. Rust's `Url` reports what the scheme implies, which is a different
/// question and the wrong one here.
fn explicit_port(candidate: &str) -> Option<String> {
    let after_scheme = candidate
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(candidate);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    // An IPv6 literal carries colons of its own; only one after the closing
    // bracket is a port.
    let tail = match authority.rfind(']') {
        Some(bracket) => &authority[bracket + 1..],
        None => authority,
    };
    let port = tail.rsplit_once(':')?.1;
    port.parse::<u16>().ok().map(|value| format!(":{value}"))
}

pub(crate) fn fingerprint(server: &str, username: &str, password: &str) -> String {
    let raw = server.trim();
    let candidate = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let normalized = Url::parse(&candidate)
        .ok()
        .map(|url| {
            let host = url.host_str().unwrap_or(raw).to_lowercase();
            // Read from the text the listener typed, not from `Url`. The
            // crate normalizes a scheme's default port away, so `:443` on an
            // https profile vanished here and survived on iOS, and the two
            // fingerprints then never matched. Step 4 of the contract is to
            // preserve the port that was written, whatever it is.
            let port = explicit_port(&candidate).unwrap_or_default();
            let mut path = url.path().trim_end_matches('/').to_lowercase();
            if path.ends_with("/rest") {
                path.truncate(path.len() - 5)
            }
            format!("{host}{port}{path}")
        })
        .unwrap_or_else(|| raw.to_lowercase());
    let digest =
        Sha256::digest(format!("{normalized}|{}|{password}", username.to_lowercase()).as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        clock_estimate, drain_frames, fingerprint, valid_command, valid_handoff, valid_peer,
        ConnectCommand, ConnectCommitment, ConnectGroup, ConnectGroupJoin, ConnectGroupReply,
        ConnectHandoff, ConnectPeer, ConnectPlayback, ConnectTimeProbe, WireMessage,
        MAX_BUFFERED_FRAME, MAX_HANDOFF_TRACKS,
    };

    fn playing_peer() -> ConnectPeer {
        ConnectPeer {
            id: "peer-1".into(),
            name: "Living Room".into(),
            platform: "tvOS".into(),
            playback: ConnectPlayback {
                track_id: Some("track-1".into()),
                title: Some("Title".into()),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                cover_art_id: Some("cover-1".into()),
                is_playing: true,
                position: 42.25,
                duration: 218.0,
            },
            updated_at: 0.0,
            commitment: None,
        }
    }

    fn handoff() -> ConnectHandoff {
        ConnectHandoff {
            track_ids: vec!["one".into(), "two".into()],
            current_track_id: "two".into(),
            position: 42.25,
            is_playing: true,
        }
    }

    fn group() -> ConnectGroup {
        ConnectGroup {
            id: "room".into(),
            leader_id: "desktop".into(),
            track_id: "two".into(),
            position: 42.25,
            is_playing: true,
            sent_at: 1_800_000_000_000.0,
            revision: None,
        }
    }

    fn named(name: &str) -> ConnectCommand {
        ConnectCommand {
            name: name.into(),
            value: None,
            handoff: None,
            group: None,
            group_join: None,
            group_reply: None,
            time: None,
        }
    }

    #[test]
    fn fingerprint_matches_the_mobile_normalization_contract() {
        assert_eq!(
            fingerprint(
                "https://Music.Example.test:4533/navidrome/rest/",
                "TestUser",
                "correct horse"
            ),
            "88dd0e47cc9415102a62af212766772e6f553012eefd758fe5cf635011b18c72"
        );
        assert_eq!(
            fingerprint(
                "music.example.test:4533/navidrome",
                "testuser",
                "correct horse"
            ),
            "88dd0e47cc9415102a62af212766772e6f553012eefd758fe5cf635011b18c72"
        );
    }

    /// A port the listener typed is part of the identity even when it is the
    /// scheme's default. `Url::port` normalizes that away and iOS does not, so
    /// a profile entered as `https://host:443` fingerprinted differently on
    /// the two platforms and Connect simply never discovered anything.
    #[test]
    fn an_explicitly_written_default_port_stays_in_the_fingerprint() {
        assert_eq!(
            fingerprint(
                "https://music.example.test:443",
                "testuser",
                "correct horse"
            ),
            fingerprint("music.example.test:443", "testuser", "correct horse")
        );
        assert_ne!(
            fingerprint(
                "https://music.example.test:443",
                "testuser",
                "correct horse"
            ),
            fingerprint("https://music.example.test", "testuser", "correct horse")
        );
        // An IPv6 literal's own colons are not a port.
        assert_eq!(
            fingerprint("http://[::1]/navidrome", "testuser", "correct horse"),
            fingerprint("[::1]/navidrome", "testuser", "correct horse")
        );
        assert_ne!(
            fingerprint("http://[::1]:4533", "testuser", "correct horse"),
            fingerprint("http://[::1]", "testuser", "correct horse")
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(br#"{"kind":"stat"#);
        assert!(
            drain_frames(&mut buffer).is_empty(),
            "a partial frame yields nothing"
        );
        buffer.extend_from_slice(b"e\"}\n");
        let frames = drain_frames(&mut buffer);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], br#"{"kind":"state"}"#);
        assert!(buffer.is_empty());
    }

    #[test]
    fn several_frames_in_one_chunk_are_all_delivered() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n{\"partial\"");
        let frames = drain_frames(&mut buffer);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[2], b"{\"c\":3}");
        // The tail without a newline stays put for the next read.
        assert_eq!(buffer, b"{\"partial\"");
    }

    #[test]
    fn an_oversized_unterminated_buffer_is_discarded() {
        let mut buffer = vec![b'x'; MAX_BUFFERED_FRAME + 1];
        assert!(drain_frames(&mut buffer).is_empty());
        assert!(
            buffer.is_empty(),
            "the contract discards it rather than growing"
        );

        // A complete frame followed by a huge tail still delivers the frame.
        let mut mixed = Vec::from(&b"{\"kind\":\"state\"}\n"[..]);
        mixed.extend(std::iter::repeat_n(b'x', MAX_BUFFERED_FRAME + 1));
        let frames = drain_frames(&mut mixed);
        assert_eq!(frames.len(), 1);
        assert!(mixed.is_empty());
    }

    #[test]
    fn a_frame_carrying_the_wrong_fingerprint_is_still_decodable_but_distinguishable() {
        // Authentication is compared after decoding, so the decoder must not be
        // what rejects a foreign frame — `handle_wire` is, by string equality.
        let frame = br#"{"kind":"command","authentication":"not-ours","command":{"name":"pause"}}"#;
        let message: WireMessage = serde_json::from_slice(frame).expect("decodes");
        assert_eq!(message.authentication, "not-ours");
        assert_ne!(message.authentication, fingerprint("host", "user", "pass"));
    }

    #[test]
    fn invalid_json_is_skipped_without_disturbing_the_frames_around_it() {
        let mut buffer = Vec::new();
        buffer
            .extend_from_slice(b"not json at all\n{\"kind\":\"state\",\"authentication\":\"x\"}\n");
        let frames = drain_frames(&mut buffer);
        assert_eq!(frames.len(), 2);
        assert!(serde_json::from_slice::<WireMessage>(&frames[0]).is_err());
        assert!(serde_json::from_slice::<WireMessage>(&frames[1]).is_ok());
    }

    #[test]
    fn peer_validation_rejects_the_shapes_that_would_corrupt_the_device_list() {
        assert!(valid_peer(&playing_peer()));

        let mut nameless = playing_peer();
        nameless.name = String::new();
        assert!(!valid_peer(&nameless));

        let mut anonymous = playing_peer();
        anonymous.id = String::new();
        assert!(!valid_peer(&anonymous));

        let mut backwards = playing_peer();
        backwards.playback.position = -1.0;
        assert!(!valid_peer(&backwards));

        let mut unreal = playing_peer();
        unreal.playback.duration = f64::NAN;
        assert!(!valid_peer(&unreal));
    }

    #[test]
    fn command_validation_matches_the_wire_contract() {
        for name in ["play", "pause", "toggle", "previous", "next"] {
            assert!(
                valid_command(&named(name)),
                "{name} is a v1 transport command"
            );
        }
        assert!(
            !valid_command(&named("seek")),
            "seek without a value is malformed"
        );
        assert!(
            !valid_command(&named("selfDestruct")),
            "unknown names are refused"
        );

        let mut seek = named("seek");
        seek.value = Some(42.25);
        assert!(valid_command(&seek));
        seek.value = Some(f64::INFINITY);
        assert!(!valid_command(&seek));

        let mut transfer = named("handoff");
        transfer.handoff = Some(handoff());
        assert!(valid_command(&transfer));

        let mut join = named("groupJoin");
        join.group_join = Some(ConnectGroupJoin {
            group: group(),
            handoff: handoff(),
        });
        assert!(valid_command(&join));

        let mut sync = named("groupSync");
        sync.group = Some(group());
        assert!(valid_command(&sync));
        assert!(
            !valid_command(&named("groupSync")),
            "groupSync needs its group"
        );
    }

    #[test]
    fn a_handoff_longer_than_the_cap_is_refused() {
        let mut oversized = handoff();
        oversized.track_ids = (0..MAX_HANDOFF_TRACKS + 1).map(|n| n.to_string()).collect();
        assert!(!valid_handoff(&oversized));

        let mut at_cap = handoff();
        at_cap.track_ids = (0..MAX_HANDOFF_TRACKS).map(|n| n.to_string()).collect();
        assert!(valid_handoff(&at_cap));

        let mut empty = handoff();
        empty.track_ids = Vec::new();
        assert!(!valid_handoff(&empty));
    }

    #[test]
    fn command_frames_omit_unused_optional_fields() {
        let frame = WireMessage {
            kind: "command".to_string(),
            authentication: "fingerprint".to_string(),
            peer: None,
            command: Some(ConnectCommand {
                name: "pause".to_string(),
                value: None,
                handoff: None,
                group: None,
                group_join: None,
                group_reply: None,
                time: None,
            }),
        };
        assert_eq!(
            serde_json::to_value(frame).unwrap(),
            serde_json::json!({
                "kind": "command", "authentication": "fingerprint", "command": { "name": "pause" }
            })
        );
        let handoff = ConnectCommand {
            name: "handoff".to_string(),
            value: None,
            handoff: Some(ConnectHandoff {
                track_ids: vec!["one".into(), "two".into()],
                current_track_id: "two".into(),
                position: 42.25,
                is_playing: true,
            }),
            group: None,
            group_join: None,
            group_reply: None,
            time: None,
        };
        assert_eq!(
            serde_json::to_value(handoff).unwrap()["handoff"]["currentTrackID"],
            "two"
        );

        let group = ConnectGroup {
            id: "room".into(),
            leader_id: "desktop".into(),
            track_id: "two".into(),
            position: 42.25,
            is_playing: true,
            sent_at: 1_800_000_000_000.0,
            revision: None,
        };
        let join = ConnectCommand {
            name: "groupJoin".into(),
            value: None,
            handoff: None,
            group: None,
            group_join: Some(ConnectGroupJoin {
                group,
                handoff: ConnectHandoff {
                    track_ids: vec!["one".into(), "two".into()],
                    current_track_id: "two".into(),
                    position: 42.25,
                    is_playing: true,
                },
            }),
            group_reply: None,
            time: None,
        };
        let value = serde_json::to_value(join).unwrap();
        assert_eq!(value["name"], "groupJoin");
        assert_eq!(value["groupJoin"]["group"]["leaderID"], "desktop");
        assert_eq!(value["groupJoin"]["handoff"]["trackIDs"][1], "two");
    }

    /// A request omits `t2`/`t3` and a reply carries both. Swift's synthesized
    /// optional encoding omits rather than nulls, so this must too or the two
    /// implementations stop agreeing on what a probe looks like.
    #[test]
    fn clock_probe_frames_match_the_optional_omission_contract() {
        let request = ConnectCommand {
            name: "timePing".into(),
            value: None,
            handoff: None,
            group: None,
            group_join: None,
            group_reply: None,
            time: Some(ConnectTimeProbe {
                id: "probe-1".into(),
                t1: 1_800_000_000_000.0,
                t2: None,
                t3: None,
            }),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "name": "timePing",
                "time": { "id": "probe-1", "t1": 1_800_000_000_000.0_f64 }
            })
        );

        let reply = ConnectCommand {
            name: "timePong".into(),
            value: None,
            handoff: None,
            group: None,
            group_join: None,
            group_reply: None,
            time: Some(ConnectTimeProbe {
                id: "probe-1".into(),
                t1: 1_800_000_000_000.0,
                t2: Some(1_800_000_000_012.0),
                t3: Some(1_800_000_000_013.0),
            }),
        };
        let value = serde_json::to_value(reply).unwrap();
        assert_eq!(value["time"]["t2"], 1_800_000_000_012.0_f64);
        assert_eq!(value["time"]["t3"], 1_800_000_000_013.0_f64);
    }

    #[test]
    fn a_clock_probe_needs_an_id_and_a_finite_send_time() {
        let mut ping = named("timePing");
        assert!(!valid_command(&ping), "timePing needs its probe");
        ping.time = Some(ConnectTimeProbe {
            id: String::new(),
            t1: 1.0,
            t2: None,
            t3: None,
        });
        assert!(
            !valid_command(&ping),
            "an anonymous probe cannot be matched"
        );
        ping.time = Some(ConnectTimeProbe {
            id: "probe-1".into(),
            t1: f64::NAN,
            t2: None,
            t3: None,
        });
        assert!(
            !valid_command(&ping),
            "a probe with no send time measures nothing"
        );
        ping.time = Some(ConnectTimeProbe {
            id: "probe-1".into(),
            t1: 1_800_000_000_000.0,
            t2: None,
            t3: None,
        });
        assert!(valid_command(&ping));
    }

    /// A reply has to name both the session and the device, or a leader
    /// cannot match it to an invitation and is back to guessing.
    #[test]
    fn a_group_reply_needs_a_session_and_a_device() {
        let mut accept = named("groupAccept");
        assert!(!valid_command(&accept), "an accept needs its reply");
        accept.group_reply = Some(ConnectGroupReply {
            session_id: String::new(),
            device_id: "phone".into(),
            revision: 1,
            reason: None,
        });
        assert!(
            !valid_command(&accept),
            "a reply to no session means nothing"
        );
        accept.group_reply = Some(ConnectGroupReply {
            session_id: "room".into(),
            device_id: String::new(),
            revision: 1,
            reason: None,
        });
        assert!(
            !valid_command(&accept),
            "an anonymous reply cannot be matched"
        );
        accept.group_reply = Some(ConnectGroupReply {
            session_id: "room".into(),
            device_id: "phone".into(),
            revision: 1,
            reason: None,
        });
        assert!(valid_command(&accept));

        let mut decline = named("groupDecline");
        decline.group_reply = Some(ConnectGroupReply {
            session_id: "room".into(),
            device_id: "phone".into(),
            revision: 1,
            reason: Some("in another session".into()),
        });
        assert!(valid_command(&decline));
    }

    /// An accept omits the decline-only reason rather than sending null, and a
    /// group frame from an updated leader carries its revision.
    #[test]
    fn session_frames_match_the_optional_omission_contract() {
        let accept = ConnectCommand {
            name: "groupAccept".into(),
            value: None,
            handoff: None,
            group: None,
            group_join: None,
            group_reply: Some(ConnectGroupReply {
                session_id: "room".into(),
                device_id: "phone".into(),
                revision: 3,
                reason: None,
            }),
            time: None,
        };
        assert_eq!(
            serde_json::to_value(accept).unwrap(),
            serde_json::json!({
                "name": "groupAccept",
                "groupReply": { "sessionID": "room", "deviceID": "phone", "revision": 3 }
            })
        );
    }

    /// A peer frame from a client that predates the commitment field must
    /// still decode, or updating one device makes the other invisible to it.
    #[test]
    fn a_peer_without_a_commitment_still_decodes() {
        let frame = br#"{"id":"phone","name":"iPhone","platform":"iPhone","updatedAt":808315200.0,
            "playback":{"isPlaying":true,"position":1.0,"duration":2.0}}"#;
        let peer: ConnectPeer = serde_json::from_slice(frame).unwrap();
        assert!(valid_peer(&peer));
        assert!(peer.commitment.is_none());
    }

    /// Likewise a group frame with no revision: absent is read as 0, which is
    /// what keeps an un-updated leader's frames acceptable.
    #[test]
    fn a_group_frame_without_a_revision_still_decodes() {
        let frame = br#"{"id":"room","leaderID":"mac","trackID":"one","position":1.0,
            "isPlaying":true,"sentAt":1800000000000.0}"#;
        let group: ConnectGroup = serde_json::from_slice(frame).unwrap();
        assert_eq!(group.revision, None);
    }

    /// The arithmetic the whole group clock rests on. A peer 500 ms ahead over
    /// a 20 ms round trip reads as exactly that, and ten seconds spent inside
    /// the responder cancels out rather than becoming apparent clock skew.
    #[test]
    fn clock_estimate_measures_offset_and_ignores_responder_delay() {
        let (offset, round_trip) = clock_estimate(1_000.0, 1_510.0, 1_520.0, 1_030.0).unwrap();
        assert!((offset - 500.0).abs() < 0.001);
        assert!((round_trip - 20.0).abs() < 0.001);

        let (slow_offset, slow_round_trip) =
            clock_estimate(1_000.0, 1_510.0, 11_510.0, 11_020.0).unwrap();
        assert!((slow_offset - 500.0).abs() < 0.001);
        assert!((slow_round_trip - 20.0).abs() < 0.001);

        // A peer behind this device reads negative. The sign is added to the
        // local clock, so reversing it would double the error.
        let (behind, _) = clock_estimate(1_000.0, 710.0, 720.0, 1_030.0).unwrap();
        assert!((behind + 300.0).abs() < 0.001);

        // Timestamps that cannot have happened in that order describe no round
        // trip, so there is nothing in them to believe.
        assert!(clock_estimate(1_000.0, 1_010.0, 1_020.0, 1_005.0).is_none());
    }

    /// The fixture that would have caught the casing split, and the reason it
    /// is written as literal JSON rather than a round trip through these
    /// structs. Both sides passed their own tests for as long as the desktop
    /// has existed, because each asserted the shape it already produced.
    /// `docs/connect/WIRE-V1.md` names `SplyntConnect.swift` as the contract,
    /// and Swift's synthesized coding keys spell every id key with a capital
    /// `ID`. These strings are copied from that document; change them only
    /// when the Swift side changes first.
    #[test]
    fn wire_frames_use_the_swift_id_spelling() {
        let mut peer = playing_peer();
        peer.updated_at = 808_315_200.0;
        peer.commitment = Some(ConnectCommitment {
            session_id: Some("room".into()),
            leader_id: Some("mac".into()),
            revision: 4,
            controlling_peer_id: None,
        });
        let state = serde_json::to_value(&peer).unwrap();
        assert_eq!(state["playback"]["trackID"], "track-1");
        assert_eq!(state["playback"]["coverArtID"], "cover-1");
        assert_eq!(state["commitment"]["sessionID"], "room");
        assert_eq!(state["commitment"]["leaderID"], "mac");
        assert!(
            state["playback"].get("trackId").is_none(),
            "the camelCase spelling is not on this wire"
        );

        let mut transfer = named("handoff");
        transfer.handoff = Some(handoff());
        let frame = serde_json::to_value(&transfer).unwrap();
        assert_eq!(frame["handoff"]["trackIDs"][0], "one");
        assert_eq!(frame["handoff"]["currentTrackID"], "two");

        // And the decode direction, from frames shaped exactly as iOS writes
        // them. A missing key here does not fail loudly: an `Option` reads as
        // absent and a required field kills the whole frame, which is what
        // made a phone and a Mac see each other as devices playing nothing.
        let ios_state =
            br#"{"id":"phone","name":"iPhone","platform":"iPhone","updatedAt":808315200.0,
            "playback":{"trackID":"t","title":"Title","artist":"Artist","album":"Album",
            "coverArtID":"c","isPlaying":true,"position":42.25,"duration":218.0},
            "commitment":{"revision":2,"controllingPeerID":"mac"}}"#;
        let decoded: ConnectPeer = serde_json::from_slice(ios_state).unwrap();
        assert_eq!(decoded.playback.track_id.as_deref(), Some("t"));
        assert_eq!(decoded.playback.cover_art_id.as_deref(), Some("c"));
        assert_eq!(
            decoded.commitment.unwrap().controlling_peer_id.as_deref(),
            Some("mac"),
            "a peer driving another device must not read as idle"
        );

        let ios_join = br#"{"name":"groupJoin","groupJoin":{
            "group":{"id":"room","leaderID":"phone","trackID":"two","position":42.25,
            "isPlaying":true,"sentAt":1800000000000.0,"revision":1},
            "handoff":{"trackIDs":["one","two"],"currentTrackID":"two","position":42.25,
            "isPlaying":true}}}"#;
        let join: ConnectCommand = serde_json::from_slice(ios_join).unwrap();
        assert!(valid_command(&join));
        let join = join.group_join.unwrap();
        assert_eq!(join.group.leader_id, "phone");
        assert_eq!(join.handoff.current_track_id, "two");

        let ios_accept = br#"{"name":"groupAccept","groupReply":
            {"sessionID":"room","deviceID":"phone","revision":1}}"#;
        let accept: ConnectCommand = serde_json::from_slice(ios_accept).unwrap();
        assert!(
            valid_command(&accept),
            "a leader that cannot read an accept counts no members"
        );
        assert_eq!(accept.group_reply.unwrap().device_id, "phone");
    }
}
