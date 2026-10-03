import type { ConnectPeer, ConnectPlayback, RepeatMode } from "../types";

/// When an idle Splynt device starts mirroring the one that is playing, and
/// when it lets go. `docs/connect/WIRE-V1.md` states the rules, and the follow
/// scenarios in `docs/connect/wire-fixtures.json` pin them. The iOS app runs
/// the same scenarios against its Swift copy (`SplyntConnectFollow`), so the
/// two cannot drift apart without a test failing on one side.
///
/// Times are seconds on one clock of the caller's choosing.

export type FollowLocal = {
  deviceID?: string;
  /// This computer's own audio is playing.
  rendering: boolean;
  hasTrack: boolean;
  inSession: boolean;
  /// When this computer's own audio last stopped. Undefined if it has not
  /// played anything since launch.
  stoppedAt?: number;
  /// The peer this computer already follows.
  followingID?: string;
};

export type FollowPeer = {
  id: string;
  isPlaying: boolean;
  hasTrack: boolean;
  controllingPeerID?: string;
  sessionID?: string;
  leaderID?: string;
  /// When its `isPlaying` last turned true in snapshots this computer saw.
  playStartedAt?: number;
  /// When its `isPlaying` last turned false.
  stoppedAt?: number;
};

export type FollowDecision =
  | { action: "none" }
  | { action: "follow"; peerID: string }
  | { action: "detach"; reason: string };

/// A peer that just vanished still has a last frame saying it is playing. On
/// 2026-08-31 the phone re-adopted an Apple TV two seconds after losing it.
export const DROP_COOLDOWN_SECONDS = 30;

export function decideFollow(
  local: FollowLocal,
  peers: FollowPeer[],
  dropped: { id: string; until: number } | undefined,
  now: number,
): FollowDecision {
  const candidate = (peer: FollowPeer) => {
    if (!peer.isPlaying || !peer.hasTrack || peer.controllingPeerID) return false;
    // A group follower's leader overrides anything a remote sends it.
    if (peer.sessionID && peer.leaderID !== peer.id) return false;
    if (dropped && dropped.id === peer.id && now < dropped.until) return false;
    return true;
  };
  const started = (peer: FollowPeer) => peer.playStartedAt ?? Number.NEGATIVE_INFINITY;
  const latest = (list: FollowPeer[]) => list.reduce<FollowPeer | undefined>((best, peer) => {
    if (!best) return peer;
    if (started(peer) !== started(best)) return started(peer) > started(best) ? peer : best;
    return peer.id < best.id ? peer : best;
  }, undefined);

  if (local.followingID) {
    const followed = peers.find((peer) => peer.id === local.followingID);
    if (!followed) return { action: "detach", reason: "peer left the network" };
    if (followed.controllingPeerID) return { action: "detach", reason: "peer is a controller" };
    if (candidate(followed)) return { action: "none" };
    // Stay attached through a pause, as Spotify does, unless another device
    // has started playing since.
    const pausedAt = followed.stoppedAt ?? Number.NEGATIVE_INFINITY;
    const next = latest(peers.filter((peer) => peer.id !== followed.id && candidate(peer) && started(peer) > pausedAt));
    return next ? { action: "follow", peerID: next.id } : { action: "none" };
  }

  if (local.rendering || local.inSession) return { action: "none" };
  const eligible = peers.filter((peer) => {
    if (!candidate(peer)) return false;
    // A paused queue is something the listener is in the middle of. Only a
    // device that started after it stopped takes it over.
    if (!local.hasTrack || local.stoppedAt === undefined) return true;
    return started(peer) > local.stoppedAt;
  });
  const next = latest(eligible);
  return next ? { action: "follow", peerID: next.id } : { action: "none" };
}

/// When each peer started and stopped playing, from the snapshots received.
/// A peer first seen playing counts as starting then.
export type PeerActivity = Record<string, { playing: boolean; startedAt?: number; stoppedAt?: number }>;

export function notePeerActivity(previous: PeerActivity, peers: ConnectPeer[], now: number): PeerActivity {
  const next: PeerActivity = {};
  for (const peer of peers) {
    const playing = peer.playback.isPlaying && Boolean(peer.playback.trackID);
    const was = previous[peer.id];
    next[peer.id] = {
      playing,
      startedAt: playing && !was?.playing ? now : was?.startedAt,
      stoppedAt: !playing && was?.playing ? now : was?.stoppedAt,
    };
  }
  return next;
}

export function followPeers(peers: ConnectPeer[], activity: PeerActivity): FollowPeer[] {
  return peers.map((peer) => ({
    id: peer.id,
    isPlaying: peer.playback.isPlaying,
    hasTrack: Boolean(peer.playback.trackID),
    controllingPeerID: peer.commitment?.controllingPeerID,
    sessionID: peer.commitment?.sessionID,
    leaderID: peer.commitment?.leaderID,
    playStartedAt: activity[peer.id]?.startedAt,
    stoppedAt: activity[peer.id]?.stoppedAt,
  }));
}

/// False for a v1 peer, whose shuffle, repeat, volume and queue editing are
/// disabled rather than faked locally.
export function hasRemoteControl(playback: ConnectPlayback) {
  return playback.shuffle !== undefined || playback.repeatMode !== undefined || playback.volume !== undefined
    || playback.queueRevision !== undefined || playback.queueIndex !== undefined
    || playback.queueLength !== undefined || playback.contextLabel !== undefined;
}

/// A desktop peer defaults to the OS computer name, which arrives over Bonjour
/// as `Something.local`. Nobody calls their laptop that. Mirrors
/// `SplyntConnectPeer.displayName` on the Swift side.
export function deviceName(peer: Pick<ConnectPeer, "name">) {
  return peer.name.toLowerCase().endsWith(".local") ? peer.name.slice(0, -6) : peer.name;
}

const APPLE_EPOCH_MS = 978_307_200_000;

/// Where a peer's track has reached now. `updatedAt` is this computer's own
/// receive stamp in Apple reference seconds, written by the transport, and
/// each frame's position is current at the moment the peer sent it.
export function projectedPeerPosition(playback: ConnectPlayback, updatedAt: number, nowMs = Date.now()) {
  const base = Number.isFinite(playback.position) ? Math.max(0, playback.position) : 0;
  if (!playback.isPlaying || !Number.isFinite(updatedAt)) return base;
  const elapsed = Math.max(0, (nowMs - APPLE_EPOCH_MS) / 1000 - updatedAt);
  const moved = base + elapsed;
  return playback.duration > 0 ? Math.min(moved, playback.duration) : moved;
}

/// After sending something that changes the track, the target can still
/// publish a frame or two from the old one while it resolves the new.
/// Applying those flips the mirror back to the old song for a moment. The
/// caller ignores such a frame only while its mirror already shows another
/// track; until then the frame changes nothing a listener sees.
export const TRACK_GUARD_MS = 2_500;
export type TrackGuard = { oldTrackID?: string; until: number };

export function trackGuardIgnores(guard: TrackGuard | undefined, trackID: string | undefined, nowMs = Date.now()) {
  return Boolean(guard && nowMs < guard.until && trackID === guard.oldTrackID);
}

export function repeatFromWire(mode: string | undefined): RepeatMode | undefined {
  if (mode === undefined) return undefined;
  return mode === "all" || mode === "one" ? mode : "off";
}

export function repeatToWire(mode: RepeatMode) {
  return mode === "one" ? 2 : mode === "all" ? 1 : 0;
}
