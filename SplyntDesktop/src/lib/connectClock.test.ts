import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  DRIFT_DEADBAND, DRIFT_MAX_CONVERGENCE, DRIFT_SEEK_THRESHOLD,
  driftCorrection, projectedGroupPosition,
} from "./connectClock";
import type { ConnectGroup } from "../types";

/// The desktop half of the Splynt Connect group clock. The numbers here have
/// to match `SplyntConnectDrift` and `SplyntConnectGroup.projectedPosition` in
/// `SplyntConnect.swift`; a device following the same leader must reach the
/// same decision whichever client it runs.

function group(overrides: Partial<ConnectGroup> = {}): ConnectGroup {
  return {
    id: "session", leaderID: "leader", trackID: "track",
    position: 30, isPlaying: true, sentAt: Date.now(),
    ...overrides,
  };
}

describe("group position projection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-29T12:00:00Z"));
  });
  afterEach(() => vi.useRealTimers());

  /// The bug the clock probe exists for. A follower whose clock ran a second
  /// ahead of the leader read a second of transit on a frame that took
  /// milliseconds, and projected a second past where the leader actually was.
  it("stops a skewed wall clock from becoming projected position", () => {
    const frame = group({ sentAt: Date.now() - 10 });
    expect(projectedGroupPosition(frame)).toBeGreaterThan(30.9 - 0.9);
    expect(projectedGroupPosition(frame, -1_000)).toBeCloseTo(30, 2);
  });

  /// The mirror case: a follower behind the leader clamped its negative
  /// transit to zero and sat permanently late instead.
  it("stops a follower behind the leader from under-shooting", () => {
    const frame = group({ sentAt: Date.now() + 990 });
    expect(projectedGroupPosition(frame)).toBeCloseTo(30, 3);
    expect(projectedGroupPosition(frame, 1_000)).toBeCloseTo(30.01, 2);
  });

  it("clamps a frame off a stalled socket to two seconds", () => {
    expect(projectedGroupPosition(group({ sentAt: Date.now() - 45_000 }), 0)).toBeCloseTo(32, 3);
  });

  it("gives a paused leader a position rather than a clock", () => {
    expect(projectedGroupPosition(group({ isPlaying: false, position: 12 }), 0)).toBe(12);
  });
});

describe("drift correction", () => {
  /// Steady state. The product target is 10 ms of skew, so anything ignored
  /// has to be smaller than that.
  it("leaves the deadband alone", () => {
    expect(DRIFT_DEADBAND).toBeLessThan(0.01);
    expect(driftCorrection(0).kind).toBe("none");
    expect(driftCorrection(0.005).kind).toBe("none");
    expect(driftCorrection(-0.005).kind).toBe("none");
  });

  /// Ahead of the leader means slow down. Reversing this would run the two
  /// devices apart at twice the rate.
  it("slows down when ahead and speeds up when behind", () => {
    const ahead = driftCorrection(0.1);
    const behind = driftCorrection(-0.1);
    expect(ahead.kind === "converge" && ahead.rate < 1).toBe(true);
    expect(behind.kind === "converge" && behind.rate > 1).toBe(true);
  });

  /// The hold has to close the gap it was computed for: 2% held for five
  /// seconds moves the playhead 100 ms.
  it("sizes the convergence window to the measured gap", () => {
    const correction = driftCorrection(0.1);
    if (correction.kind !== "converge") throw new Error("0.1 s should converge by rate");
    expect((1 - correction.rate) * correction.seconds).toBeCloseTo(0.1, 6);
    expect(correction.seconds).toBeLessThanOrEqual(DRIFT_MAX_CONVERGENCE);
  });

  it("seeks only beyond the convergence budget", () => {
    expect(DRIFT_SEEK_THRESHOLD).toBeCloseTo(0.4, 6);
    expect(driftCorrection(0.39).kind).toBe("converge");
    expect(driftCorrection(0.41).kind).toBe("seek");
    expect(driftCorrection(-3).kind).toBe("seek");
    // v1's entire correction range now converges silently instead.
    expect(driftCorrection(0.3).kind).toBe("converge");
  });

  it("ignores a meaningless measurement", () => {
    expect(driftCorrection(Number.NaN).kind).toBe("none");
  });
});
