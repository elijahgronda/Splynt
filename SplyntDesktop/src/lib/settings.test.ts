import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  acceptsBitRateCap, defaultSettings, effectiveStreamQuality, homeRows, readSettings, transcodes,
} from "./settings";

describe("desktop settings", () => {
  beforeEach(() => localStorage.clear());

  it("falls back to the defaults for an unset profile", () => {
    expect(readSettings("nobody@nowhere")).toEqual(defaultSettings);
  });

  it("keeps Original as no transcode and no bitrate cap", () => {
    expect(transcodes(0)).toBe(false);
    expect(transcodes(320)).toBe(true);
    // Asking for wav plus a cap is the combination that yields no audio.
    expect(acceptsBitRateCap("wav")).toBe(false);
    expect(acceptsBitRateCap("opus")).toBe(true);
  });

  it("lets data saver override the chosen tier without editing it", () => {
    const settings = { ...defaultSettings, streamQuality: 320 as const, dataSaver: true };
    expect(effectiveStreamQuality(settings)).toBe(128);
    expect(effectiveStreamQuality({ ...settings, dataSaver: false })).toBe(320);
  });

  it("rejects a stored quality tier the app does not offer", () => {
    localStorage.setItem("splice.settings.v1:one", JSON.stringify({ streamQuality: 192, streamFormat: "flac" }));
    const settings = readSettings("one");
    expect(settings.streamQuality).toBe(defaultSettings.streamQuality);
    expect(settings.streamFormat).toBe(defaultSettings.streamFormat);
  });

  it("drops unknown home rows and appends ones added since the order was saved", () => {
    localStorage.setItem("splice.settings.v1:two", JSON.stringify({ homeRowOrder: ["discover", "discover", "dailyMixes"], hiddenHomeRows: ["onRepeat", "onRepeat"] }));
    const settings = readSettings("two");
    expect(settings.homeRowOrder[0]).toBe("discover");
    expect(settings.homeRowOrder).not.toContain("dailyMixes");
    expect([...settings.homeRowOrder].sort()).toEqual([...homeRows].sort());
    expect(settings.hiddenHomeRows).toEqual(["onRepeat"]);
  });

  it("clamps crossfade to the supported range", () => {
    localStorage.setItem("splice.settings.v1:three", JSON.stringify({ crossfadeSeconds: 99 }));
    expect(readSettings("three").crossfadeSeconds).toBe(12);
  });

  it("defaults invalid lyric sources to server-first automatic lookup", () => {
    localStorage.setItem("splice.settings.v1:lyrics", JSON.stringify({ lyricsSource: "unknown" }));
    expect(readSettings("lyrics").lyricsSource).toBe("auto");
  });
});
