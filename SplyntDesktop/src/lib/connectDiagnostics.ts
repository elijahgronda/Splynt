import { invoke } from "@tauri-apps/api/core";

/// Splynt Connect is the one part of the client that cannot be debugged by
/// looking at it: the symptom lives in the relationship between two machines,
/// and the person who hits it is not going to have a debugger attached to
/// both. So it records measurements to the same diagnostics file as everything
/// else, and the numbers are the point — drift and clock skew are only useful
/// if they can be read back as numbers rather than parsed out of a sentence.
///
/// What it deliberately does NOT do is trace. Group sync runs at 1 Hz *per
/// peer*, so an hour with three of them is around eleven thousand ticks; a line
/// each would blow through the log's 2 MB rotation in one session and take the
/// cross-launch timeline — the reason the log exists — with it. Instead the
/// per-tick numbers are folded into a bucket and one summary line is written
/// every 30 seconds. Individual lines are reserved for decisions (a correction
/// fired) and for lifecycle, both of which are rare enough to read.
///
/// The authentication fingerprint never appears here. It is a replayable
/// bearer secret on the LAN and the v1 wire contract says to keep it out of
/// logs and diagnostics. Neither do track id lists: a handoff carries up to a
/// thousand of them and the count is the only part worth having.

const SUMMARY_INTERVAL_MS = 30_000;
/// A correction should be rare now. If something makes them constant, the
/// bucket's `corrections` count still tells the story without one line each.
const CORRECTION_LOG_INTERVAL_MS = 5_000;

type Detail = Record<string, string | number | boolean>;

export function logConnectEvent(event: string, detail: Detail = {}) {
  void invoke("log_event", { event: `connect_${event}`, detail }).catch(() => undefined);
}

type Bucket = {
  sessionId: string;
  since: number;
  samples: number;
  driftMin: number;
  driftMax: number;
  driftTotal: number;
  converges: number;
  seeks: number;
  trackChanges: number;
  skewMin: number;
  skewMax: number;
  skewTotal: number;
  offsetSamples: number;
  offsetMin: number;
  offsetMax: number;
  offsetTotal: number;
};

let bucket: Bucket | undefined;
let lastCorrectionLog = 0;

function freshBucket(sessionId: string): Bucket {
  return {
    sessionId,
    since: Date.now(),
    samples: 0,
    driftMin: Number.POSITIVE_INFINITY,
    driftMax: 0,
    driftTotal: 0,
    converges: 0,
    seeks: 0,
    trackChanges: 0,
    skewMin: Number.POSITIVE_INFINITY,
    skewMax: 0,
    skewTotal: 0,
    offsetSamples: 0,
    offsetMin: Number.POSITIVE_INFINITY,
    offsetMax: Number.NEGATIVE_INFINITY,
    offsetTotal: 0,
  };
}

function round(value: number, places = 3) {
  const factor = 10 ** places;
  return Math.round(value * factor) / factor;
}

/// One line per bucket, or nothing if the session produced no samples.
function flush(reason: string) {
  const current = bucket;
  if (!current || current.samples === 0) return;
  logConnectEvent("group_drift", {
    reason,
    session: current.sessionId,
    seconds: round((Date.now() - current.since) / 1000, 1),
    samples: current.samples,
    driftMin: round(current.driftMin),
    driftMean: round(current.driftTotal / current.samples),
    driftMax: round(current.driftMax),
    // Split because they cost the listener different things: a converge is
    // inaudible, a seek is a jump. Seeks climbing is the signal that something
    // upstream is wrong.
    converges: current.converges,
    seeks: current.seeks,
    trackChanges: current.trackChanges,
    // Arrival skew: transit plus whatever the two wall clocks disagree by.
    // Kept because it is what v1 measured and what the pre-clock exports show,
    // so old and new sessions stay comparable.
    skewMin: round(current.skewMin),
    skewMean: round(current.skewTotal / current.samples),
    skewMax: round(current.skewMax),
    // The measured clock difference itself. A wide spread here means the probe
    // is being answered under load and the drift numbers built on it deserve
    // less trust.
    offsetSamples: current.offsetSamples,
    offsetMin: current.offsetSamples === 0 ? "none" : round(current.offsetMin, 1),
    offsetMean: current.offsetSamples === 0 ? "none" : round(current.offsetTotal / current.offsetSamples, 1),
    offsetMax: current.offsetSamples === 0 ? "none" : round(current.offsetMax, 1),
  });
  bucket = freshBucket(current.sessionId);
}

/// Folds one leader frame into the current bucket. Called once per sync tick,
/// so it must stay allocation-light and must never log on its own.
export function noteGroupSample(
  sessionId: string,
  sample: { drift: number; skew: number; offset?: number; correction: "none" | "converge" | "seek" },
) {
  if (!bucket || bucket.sessionId !== sessionId) {
    flush("session-change");
    bucket = freshBucket(sessionId);
  }
  const drift = Math.abs(sample.drift);
  bucket.samples += 1;
  bucket.driftTotal += drift;
  if (drift < bucket.driftMin) bucket.driftMin = drift;
  if (drift > bucket.driftMax) bucket.driftMax = drift;
  bucket.skewTotal += sample.skew;
  if (sample.skew < bucket.skewMin) bucket.skewMin = sample.skew;
  if (sample.skew > bucket.skewMax) bucket.skewMax = sample.skew;
  if (sample.offset !== undefined) {
    bucket.offsetSamples += 1;
    bucket.offsetTotal += sample.offset;
    if (sample.offset < bucket.offsetMin) bucket.offsetMin = sample.offset;
    if (sample.offset > bucket.offsetMax) bucket.offsetMax = sample.offset;
  }
  if (sample.correction === "converge") bucket.converges += 1;

  if (sample.correction === "seek") {
    bucket.seeks += 1;
    const now = Date.now();
    // Only the audible correction is worth a line. A converge happens
    // constantly by design and lives in the summary's count.
    if (now - lastCorrectionLog >= CORRECTION_LOG_INTERVAL_MS) {
      lastCorrectionLog = now;
      logConnectEvent("group_correction", {
        session: sessionId,
        drift: round(sample.drift),
        skew: round(sample.skew),
        offset: sample.offset === undefined ? "none" : round(sample.offset, 1),
      });
    }
  }

  if (Date.now() - bucket.since >= SUMMARY_INTERVAL_MS) flush("interval");
}

/// The leader moved to a track this device had to load. Counted separately
/// because the drift around a track change has a different cause from steady
/// drift, and averaging them together hides both.
export function noteGroupTrackChange(sessionId: string) {
  if (bucket?.sessionId === sessionId) bucket.trackChanges += 1;
}

/// Writes the partial bucket a session would otherwise take with it.
export function endGroupDiagnostics(reason: string) {
  flush(reason);
  bucket = undefined;
  // The rate limit exists to stop one session streaming correction lines. A
  // new session's first correction is worth seeing straight away, so it does
  // not inherit the last one's timer.
  lastCorrectionLog = 0;
}
