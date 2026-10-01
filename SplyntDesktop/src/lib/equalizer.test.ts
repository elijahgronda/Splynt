import { describe, expect, it } from "vitest";
import {
  clampEqualizer,
  equalizerBands,
  equalizerPresets,
  flatEqualizer,
  isEqualizerActive,
  isEqualizerFlat,
  matchingPresetId,
  maxBandGainDb,
  maxPreampDb,
  bandFilterType,
  responseCurveDb,
  responseDbAt,
  suggestedPreampDb,
  type EqualizerSettings,
} from "./equalizer";

function withGains(pairs: Record<number, number>, preampDb = 0): EqualizerSettings {
  return {
    enabled: true,
    preampDb,
    gainsDb: equalizerBands.map((_, index) => pairs[index] ?? 0),
  };
}

describe("equalizer response", () => {
  it("puts a band's nominal gain at that band's frequency", () => {
    expect(responseDbAt(1_000, withGains({ 5: 6 }))).toBeCloseTo(6, 1);
  });

  it("leaves distant bands alone", () => {
    const settings = withGains({ 5: 12 });
    expect(Math.abs(responseDbAt(125, settings))).toBeLessThan(1);
    expect(Math.abs(responseDbAt(8_000, settings))).toBeLessThan(1);
  });

  it("mirrors a cut against a boost", () => {
    expect(responseDbAt(500, withGains({ 4: 8 })) + responseDbAt(500, withGains({ 4: -8 }))).toBeCloseTo(0, 5);
  });

  // The reason the end bands are shelves: a peaking filter at 32 Hz would
  // leave 20 Hz almost untouched, which is not what the slider promises.
  it("lifts everything below the low shelf", () => {
    const settings = withGains({ 0: 8 });
    expect(responseDbAt(20, settings)).toBeGreaterThan(6);
    expect(Math.abs(responseDbAt(1_000, settings))).toBeLessThan(1);
  });

  it("lifts everything above the high shelf", () => {
    const settings = withGains({ [equalizerBands.length - 1]: 8 });
    expect(responseDbAt(19_000, settings)).toBeGreaterThan(6);
    expect(Math.abs(responseDbAt(1_000, settings))).toBeLessThan(1);
  });

  it("shifts the whole curve by the preamp", () => {
    expect(responseDbAt(1_000, withGains({}, -3))).toBeCloseTo(-3, 5);
  });

  it("passes a band at or above Nyquist straight through", () => {
    const top = equalizerBands.length - 1;
    // 16 kHz against a 32 kHz stream, which is what a Provider transcode can
    // hand back through octo-fiesta.
    expect(responseDbAt(10_000, withGains({ [top]: 10 }), 32_000)).toBeCloseTo(0, 5);
  });

  it("uses shelves at the ends and peaks in between", () => {
    expect(bandFilterType(0)).toBe("lowshelf");
    expect(bandFilterType(equalizerBands.length - 1)).toBe("highshelf");
    expect(bandFilterType(4)).toBe("peaking");
  });

  it("returns a curve that agrees with single-point evaluation", () => {
    const settings = withGains({ 2: 9, 7: -5 });
    const curve = responseCurveDb(settings, { points: 3, minHz: 100, maxHz: 10_000 });
    expect(curve).toHaveLength(3);
    // The middle of a log sweep from 100 to 10k is 1 kHz.
    expect(curve[1]).toBeCloseTo(responseDbAt(1_000, settings), 5);
  });

  it("produces a finite curve at full boost on every band", () => {
    const settings: EqualizerSettings = {
      enabled: true,
      preampDb: maxPreampDb,
      gainsDb: equalizerBands.map(() => maxBandGainDb),
    };
    expect(responseCurveDb(settings).every(Number.isFinite)).toBe(true);
  });
});

describe("equalizer settings", () => {
  it("clamps gains and preamp into range", () => {
    const settings = clampEqualizer({ enabled: true, preampDb: 99, gainsDb: [40, -40] });
    expect(settings.gainsDb[0]).toBe(maxBandGainDb);
    expect(settings.gainsDb[1]).toBe(-maxBandGainDb);
    expect(settings.preampDb).toBe(maxPreampDb);
  });

  it("repairs a curve of the wrong length", () => {
    expect(clampEqualizer({ gainsDb: [3, 4] }).gainsDb).toHaveLength(equalizerBands.length);
    expect(clampEqualizer({ gainsDb: new Array(40).fill(1) }).gainsDb).toHaveLength(equalizerBands.length);
    expect(clampEqualizer({ gainsDb: [3, 4] }).gainsDb[2]).toBe(0);
  });

  it("replaces values that are not numbers", () => {
    const settings = clampEqualizer({ preampDb: Number.NaN, gainsDb: [Number.POSITIVE_INFINITY, "loud", null] });
    expect(settings.preampDb).toBe(0);
    expect(settings.gainsDb.every((gain) => gain === 0)).toBe(true);
  });

  it("survives a blob with nothing in it", () => {
    expect(clampEqualizer(undefined)).toEqual(flatEqualizer);
    expect(clampEqualizer({})).toEqual(flatEqualizer);
  });

  // An enabled but flat equalizer must not route audio through Web Audio: that
  // routing cannot be undone for the element's lifetime, so "on and doing
  // nothing" has to cost the same as "off".
  it("is only active when it would change the sound", () => {
    expect(isEqualizerActive({ ...flatEqualizer, enabled: true })).toBe(false);
    expect(isEqualizerActive({ ...withGains({ 4: 3 }), enabled: false })).toBe(false);
    expect(isEqualizerActive(withGains({ 4: 3 }))).toBe(true);
    expect(isEqualizerActive(withGains({}, -3))).toBe(true);
    expect(isEqualizerFlat(flatEqualizer)).toBe(true);
  });

  it("suggests a preamp that clears the loudest boost", () => {
    expect(suggestedPreampDb(withGains({ 2: 7, 5: 4 }))).toBe(-7);
    expect(suggestedPreampDb(withGains({ 2: -7 }))).toBe(0);
  });

  it("recognises every preset from its own curve", () => {
    for (const preset of equalizerPresets) {
      expect(preset.gainsDb).toHaveLength(equalizerBands.length);
      expect(matchingPresetId(preset.gainsDb)).toBe(preset.id);
      expect(preset.gainsDb.every((gain) => Math.abs(gain) <= maxBandGainDb)).toBe(true);
    }
  });

  it("calls a hand-made curve custom", () => {
    expect(matchingPresetId(equalizerBands.map((_, index) => (index === 0 ? 11 : 0)))).toBeUndefined();
  });
});
