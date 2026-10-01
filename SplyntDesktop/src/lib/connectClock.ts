import type { ConnectGroup } from "../types";

/// The Splynt Connect group clock, mirroring `SplyntConnectDrift` and
/// `SplyntConnectGroup.projectedPosition` in `SplyntConnect.swift`. All three
/// clients have to agree on what a group frame means and on what a given drift
/// costs, so the numbers live in one place per platform and the two places
/// carry the same values.

/// Leave anything under this alone. It sits above the residual error of a LAN
/// clock estimate, so correction does not chase its own noise, and below the
/// product's 10 ms steady-state target.
export const DRIFT_DEADBAND = 0.008;
/// 2% is inaudible through a pitch-preserving rate change and still closes a
/// tenth of a second in five.
export const DRIFT_RATE_DELTA = 0.02;
/// Converging for longer than this leaves the room out of step for most of a
/// verse. Past it, pay for the audible seek.
export const DRIFT_MAX_CONVERGENCE = 20;
/// 0.4 s. The largest gap 2% can close inside the convergence budget.
export const DRIFT_SEEK_THRESHOLD = DRIFT_RATE_DELTA * DRIFT_MAX_CONVERGENCE;

export type DriftCorrection =
  | { kind: "none" }
  | { kind: "converge"; rate: number; seconds: number }
  | { kind: "seek" };

/// `drift` is this device's position minus the leader's, so a positive value
/// means it is ahead and has to slow down.
///
/// v1 corrected nothing under 1.25 s and then jumped. That window was never a
/// tolerance: a seek is audible, so correcting 40 ms with one costs more than
/// the drift does. Nudging the rate is free, which is what lets the deadband
/// fall to single-digit milliseconds.
export function driftCorrection(drift: number): DriftCorrection {
  if (!Number.isFinite(drift)) return { kind: "none" };
  const magnitude = Math.abs(drift);
  if (magnitude <= DRIFT_DEADBAND) return { kind: "none" };
  if (magnitude > DRIFT_SEEK_THRESHOLD) return { kind: "seek" };
  return {
    kind: "converge",
    rate: drift > 0 ? 1 - DRIFT_RATE_DELTA : 1 + DRIFT_RATE_DELTA,
    seconds: magnitude / DRIFT_RATE_DELTA,
  };
}

/// Where the leader's playhead has reached by now.
///
/// `offsetMs` comes from the snapshot's `clockOffsets` and is what turns the
/// transit term into a measurement. Pass undefined and this is a subtraction
/// between two wall clocks that have never agreed on the time, which is what
/// every client did before the probe existed: a follower running a second
/// ahead of the leader read a second of transit on a frame that took
/// milliseconds, and one running behind read none at all and sat late.
///
/// The two-second ceiling stays. It no longer hides clock skew, but a frame
/// really can arrive late off a stalled socket, and by then the leader has
/// moved on rather than being two seconds further into the same spot.
export function projectedGroupPosition(group: ConnectGroup, offsetMs?: number) {
  if (!group.isPlaying) return Math.max(0, group.position);
  const leaderNow = Date.now() + (offsetMs ?? 0);
  const transit = Math.min(2, Math.max(0, leaderNow - group.sentAt) / 1000);
  return Math.max(0, group.position + transit);
}
