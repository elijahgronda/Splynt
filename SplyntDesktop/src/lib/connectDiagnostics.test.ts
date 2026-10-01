import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { endGroupDiagnostics, noteGroupSample, noteGroupTrackChange } = await import("./connectDiagnostics");

function events(name: string) {
  return invoke.mock.calls
    .filter((call) => call[0] === "log_event" && call[1]?.event === `connect_${name}`)
    .map((call) => call[1].detail as Record<string, number | string>);
}

describe("Connect group diagnostics", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    // Drop whatever the previous test left in the bucket, then forget that
    // flush so it does not count as this test's output.
    endGroupDiagnostics("test-reset");
    invoke.mockClear();
    vi.useFakeTimers();
    // Far enough forward that the correction rate limiter starts clear.
    vi.setSystemTime(new Date("2026-08-27T00:00:00Z"));
  });

  it("stays silent through a stretch of ordinary sync ticks", () => {
    // Group sync runs at 1 Hz per peer. Twenty-nine seconds of it must not
    // produce twenty-nine lines, or an hour-long session buries the log.
    for (let second = 0; second < 29; second += 1) {
      noteGroupSample("session-a", { drift: 0.04, skew: 12, correction: "none" });
      vi.setSystemTime(Date.now() + 1_000);
    }
    expect(events("group_drift")).toHaveLength(0);
  });

  it("summarises the window as one line with the real distribution", () => {
    const drifts = [0.1, -0.4, 0.2];
    for (const drift of drifts) {
      noteGroupSample("session-a", { drift, skew: 20, correction: "none" });
    }
    vi.setSystemTime(Date.now() + 31_000);
    noteGroupSample("session-a", { drift: 0.3, skew: 40, correction: "none" });

    const summaries = events("group_drift");
    expect(summaries).toHaveLength(1);
    const summary = summaries[0];
    expect(summary.samples).toBe(4);
    // Drift is summarised by magnitude — a device half a second behind and one
    // half a second ahead are the same amount of wrong.
    expect(summary.driftMin).toBe(0.1);
    expect(summary.driftMax).toBe(0.4);
    expect(summary.driftMean).toBe(0.25);
    expect(summary.skewMin).toBe(20);
    expect(summary.skewMax).toBe(40);
    expect(summary.seeks).toBe(0);
  });

  it("rate-limits correction lines but never loses the count", () => {
    for (let second = 0; second < 10; second += 1) {
      noteGroupSample("session-a", { drift: 3, skew: 15, correction: "seek" });
      vi.setSystemTime(Date.now() + 1_000);
    }
    // One line at most every five seconds, while the summary keeps all ten.
    expect(events("group_correction").length).toBeLessThanOrEqual(3);
    endGroupDiagnostics("test-end");
    expect(events("group_drift")[0].seeks).toBe(10);
  });

  it("counts track changes apart from steady drift", () => {
    noteGroupSample("session-a", { drift: 0.1, skew: 10, correction: "none" });
    noteGroupTrackChange("session-a");
    noteGroupTrackChange("session-a");
    endGroupDiagnostics("test-end");
    expect(events("group_drift")[0].trackChanges).toBe(2);
  });

  it("closes out one session's numbers before starting the next", () => {
    noteGroupSample("session-a", { drift: 0.1, skew: 10, correction: "none" });
    noteGroupSample("session-b", { drift: 0.2, skew: 10, correction: "none" });
    const summaries = events("group_drift");
    expect(summaries).toHaveLength(1);
    expect(summaries[0].session).toBe("session-a");
  });

  it("separates inaudible convergence from an audible seek", () => {
    for (let second = 0; second < 4; second += 1) {
      noteGroupSample("session-a", { drift: 0.05, skew: 10, correction: "converge" });
      vi.setSystemTime(Date.now() + 1_000);
    }
    noteGroupSample("session-a", { drift: 2, skew: 10, correction: "seek" });
    endGroupDiagnostics("test-end");
    const summary = events("group_drift")[0];
    expect(summary.converges).toBe(4);
    expect(summary.seeks).toBe(1);
    // Four rate nudges must not have produced four log lines.
    expect(events("group_correction")).toHaveLength(1);
  });

  it("reports the measured clock offset, and says so when there is none", () => {
    noteGroupSample("session-a", { drift: 0.01, skew: 10, offset: -12.5, correction: "none" });
    noteGroupSample("session-a", { drift: 0.01, skew: 10, offset: 3.5, correction: "none" });
    // A peer that has not answered a probe contributes a drift sample but no
    // offset sample, so the two counts are deliberately different.
    noteGroupSample("session-a", { drift: 0.01, skew: 10, correction: "none" });
    endGroupDiagnostics("test-end");
    const summary = events("group_drift")[0];
    expect(summary.samples).toBe(3);
    expect(summary.offsetSamples).toBe(2);
    expect(summary.offsetMin).toBe(-12.5);
    expect(summary.offsetMax).toBe(3.5);
    expect(summary.offsetMean).toBe(-4.5);
  });

  it("says none rather than infinity when no peer answered a probe", () => {
    noteGroupSample("session-a", { drift: 0.01, skew: 10, correction: "none" });
    endGroupDiagnostics("test-end");
    expect(events("group_drift")[0].offsetMin).toBe("none");
    expect(events("group_drift")[0].offsetMean).toBe("none");
  });

  it("writes nothing for a session that produced no samples", () => {
    endGroupDiagnostics("never-started");
    expect(events("group_drift")).toHaveLength(0);
  });
});
