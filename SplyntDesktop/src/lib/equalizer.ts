// The equalizer's model and its arithmetic, with no Web Audio in it.
//
// Deliberately a port of Splynt/Player/Equalizer.swift rather than a second
// design: same ten ISO centres, same Q, same shelves at the ends, same preset
// curves. Web Audio's BiquadFilterNode implements the RBJ cookbook, which is
// what the iOS kernel implements by hand, so identical inputs give identical
// curves on both clients. A listener who dials in Bass Boost on their phone
// and on their desktop hears the same thing, and that is only true for as long
// as these numbers stay in step with the Swift file.
//
// The response maths below is duplicated from what BiquadFilterNode could tell
// us through `getFrequencyResponse`, because that needs a live AudioContext
// and live nodes: it cannot draw the curve before the graph exists, and it
// cannot be tested without a browser. This version runs anywhere.

export const equalizerBands = [32, 64, 125, 250, 500, 1_000, 2_000, 4_000, 8_000, 16_000] as const;

export const maxBandGainDb = 12;
export const maxPreampDb = 12;

/// One octave of bandwidth, which is the spacing between neighbouring centres,
/// so adjacent bands meet instead of leaving a dip or piling up.
export const peakingQ = Math.SQRT2;

export type EqualizerSettings = {
  enabled: boolean;
  preampDb: number;
  gainsDb: number[];
};

export const flatEqualizer: EqualizerSettings = {
  enabled: false,
  preampDb: 0,
  gainsDb: equalizerBands.map(() => 0),
};

export type EqualizerPreset = {
  id: string;
  title: string;
  gainsDb: number[];
};

/// Gains for 32, 64, 125, 250, 500, 1k, 2k, 4k, 8k, 16k.
export const equalizerPresets: readonly EqualizerPreset[] = [
  { id: "flat", title: "Flat", gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { id: "bassBoost", title: "Bass Boost", gainsDb: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] },
  { id: "bassReducer", title: "Bass Reducer", gainsDb: [-6, -5, -4, -2, 0, 0, 0, 0, 0, 0] },
  { id: "trebleBoost", title: "Treble Boost", gainsDb: [0, 0, 0, 0, 0, 0, 2, 4, 5, 6] },
  { id: "trebleReducer", title: "Treble Reducer", gainsDb: [0, 0, 0, 0, 0, 0, -2, -4, -5, -6] },
  { id: "vocal", title: "Vocal", gainsDb: [-2, -2, -1, 1, 3, 4, 3, 2, 0, -1] },
  { id: "acoustic", title: "Acoustic", gainsDb: [3, 3, 2, 0, 1, 1, 2, 3, 3, 2] },
  { id: "electronic", title: "Electronic", gainsDb: [4, 4, 2, 0, -1, 1, 0, 1, 3, 4] },
  { id: "hipHop", title: "Hip-Hop", gainsDb: [5, 4, 2, 3, -1, -1, 1, 0, 1, 2] },
  { id: "rock", title: "Rock", gainsDb: [4, 3, 2, 0, -1, 0, 2, 3, 4, 4] },
  { id: "jazz", title: "Jazz", gainsDb: [3, 2, 1, 2, -1, -1, 0, 1, 2, 3] },
  { id: "smallSpeakers", title: "Small Speakers", gainsDb: [-6, -4, 0, 3, 4, 3, 2, 1, -1, -3] },
];

/// The first band is a low shelf and the last is a high shelf. A peaking filter
/// at 32 Hz only moves a narrow strip around 32 Hz, so "turn the bass up" would
/// leave 20 Hz alone, which is not what the outermost slider promises.
export function bandFilterType(index: number): BiquadFilterType {
  if (index === 0) return "lowshelf";
  if (index === equalizerBands.length - 1) return "highshelf";
  return "peaking";
}

export function bandLabel(index: number): string {
  const hz = equalizerBands[index];
  if (hz === undefined) return "";
  if (hz >= 1_000) {
    const k = hz / 1_000;
    return Number.isInteger(k) ? `${k}k` : k.toFixed(1) + "k";
  }
  return String(hz);
}

/// Ranges enforced in one place, so a hand-edited localStorage blob, an older
/// build's shorter curve, and a NaN all land somewhere the filters can accept.
export function clampEqualizer(raw: unknown): EqualizerSettings {
  const value = (raw ?? {}) as Partial<EqualizerSettings>;
  const saved = Array.isArray(value.gainsDb) ? value.gainsDb : [];
  const gainsDb = equalizerBands.map((_, index) => {
    const candidate = Number(saved[index]);
    if (!Number.isFinite(candidate)) return 0;
    return Math.max(-maxBandGainDb, Math.min(maxBandGainDb, candidate));
  });
  const preamp = Number(value.preampDb);
  return {
    enabled: Boolean(value.enabled),
    preampDb: Number.isFinite(preamp) ? Math.max(-maxPreampDb, Math.min(maxPreampDb, preamp)) : 0,
    gainsDb,
  };
}

export function isEqualizerFlat(settings: EqualizerSettings): boolean {
  return settings.preampDb === 0 && settings.gainsDb.every((gain) => gain === 0);
}

/// Whether it is worth routing audio through a graph at all. An enabled but
/// flat equalizer is a no-op, and routing an element through Web Audio cannot
/// be undone for that element's lifetime, so a flat curve stays out of the way.
export function isEqualizerActive(settings: EqualizerSettings): boolean {
  return settings.enabled && !isEqualizerFlat(settings);
}

/// The preset whose curve these gains are, or undefined for a hand-made one.
/// Derived rather than stored: a stored name and a stored curve can disagree.
export function matchingPresetId(gainsDb: number[]): string | undefined {
  return equalizerPresets.find((preset) =>
    preset.gainsDb.length === gainsDb.length
    && preset.gainsDb.every((gain, index) => Math.abs(gain - gainsDb[index]) < 0.01))?.id;
}

/// The preamp that would put the loudest boosted band back at unity. Shown,
/// never applied: moving a control the listener did not touch is worse than
/// telling them what would help.
export function suggestedPreampDb(settings: EqualizerSettings): number {
  const peak = Math.max(0, ...settings.gainsDb);
  return peak > 0 ? -Math.round(peak) : 0;
}

export function formatDb(db: number): string {
  const rounded = Math.round(db * 10) / 10;
  if (rounded === 0) return "0.0 dB";
  return `${rounded > 0 ? "+" : "-"}${Math.abs(rounded).toFixed(1)} dB`;
}

// MARK: - Response curve

type Biquad = { b0: number; b1: number; b2: number; a1: number; a2: number };

const identity: Biquad = { b0: 1, b1: 0, b2: 0, a1: 0, a2: 0 };

/// RBJ cookbook, the same formulas BiquadFilterNode uses internally and the
/// same ones the iOS kernel evaluates by hand.
function design(index: number, gainDb: number, sampleRate: number): Biquad {
  const hz = equalizerBands[index];
  if (hz === undefined || !(sampleRate > 0) || hz >= sampleRate / 2) return identity;
  if (Math.abs(gainDb) < 0.001) return identity;

  const a = Math.pow(10, gainDb / 40);
  const w0 = (2 * Math.PI * hz) / sampleRate;
  const cosW0 = Math.cos(w0);
  const sinW0 = Math.sin(w0);
  const type = bandFilterType(index);

  if (type === "peaking") {
    const alpha = sinW0 / (2 * peakingQ);
    return normalise(1 + alpha * a, -2 * cosW0, 1 - alpha * a, 1 + alpha / a, -2 * cosW0, 1 - alpha / a);
  }

  // S = 1: the steepest shelf slope that stays monotonic, so the corner
  // cannot ring. This is also what Web Audio's shelves use.
  const alpha = (sinW0 / 2) * Math.SQRT2;
  const twoSqrtAAlpha = 2 * Math.sqrt(a) * alpha;
  if (type === "lowshelf") {
    return normalise(
      a * ((a + 1) - (a - 1) * cosW0 + twoSqrtAAlpha),
      2 * a * ((a - 1) - (a + 1) * cosW0),
      a * ((a + 1) - (a - 1) * cosW0 - twoSqrtAAlpha),
      (a + 1) + (a - 1) * cosW0 + twoSqrtAAlpha,
      -2 * ((a - 1) + (a + 1) * cosW0),
      (a + 1) + (a - 1) * cosW0 - twoSqrtAAlpha,
    );
  }
  return normalise(
    a * ((a + 1) + (a - 1) * cosW0 + twoSqrtAAlpha),
    -2 * a * ((a - 1) + (a + 1) * cosW0),
    a * ((a + 1) + (a - 1) * cosW0 - twoSqrtAAlpha),
    (a + 1) - (a - 1) * cosW0 + twoSqrtAAlpha,
    2 * ((a - 1) - (a + 1) * cosW0),
    (a + 1) - (a - 1) * cosW0 - twoSqrtAAlpha,
  );
}

function normalise(b0: number, b1: number, b2: number, a0: number, a1: number, a2: number): Biquad {
  if (a0 === 0 || !Number.isFinite(a0)) return identity;
  const result = { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
  return Object.values(result).every(Number.isFinite) ? result : identity;
}

function magnitude(section: Biquad, hz: number, sampleRate: number): number {
  const w = (2 * Math.PI * hz) / sampleRate;
  // e^-jw = cos w - j sin w, so the imaginary parts come out negative.
  const numRe = section.b0 + section.b1 * Math.cos(w) + section.b2 * Math.cos(2 * w);
  const numIm = -(section.b1 * Math.sin(w) + section.b2 * Math.sin(2 * w));
  const denRe = 1 + section.a1 * Math.cos(w) + section.a2 * Math.cos(2 * w);
  const denIm = -(section.a1 * Math.sin(w) + section.a2 * Math.sin(2 * w));
  const den = denRe * denRe + denIm * denIm;
  if (den <= 0) return 1;
  return Math.sqrt((numRe * numRe + numIm * numIm) / den);
}

export function responseDbAt(hz: number, settings: EqualizerSettings, sampleRate = 44_100): number {
  const linear = settings.gainsDb.reduce(
    (total, gain, index) => total * magnitude(design(index, gain, sampleRate), hz, sampleRate),
    1,
  );
  return linear > 0 ? 20 * Math.log10(linear) + settings.preampDb : -maxBandGainDb;
}

/// The whole curve, log-spaced across the audible band, for the graph above the
/// sliders. Designs the filters once and evaluates them `points` times, which
/// is the difference between a curve that keeps up with a drag and one that
/// does not.
export function responseCurveDb(
  settings: EqualizerSettings,
  { sampleRate = 44_100, points = 96, minHz = 20, maxHz = 20_000 } = {},
): number[] {
  if (points < 2 || minHz <= 0 || maxHz <= minHz) return [];
  const sections = settings.gainsDb
    .map((gain, index) => design(index, gain, sampleRate))
    .filter((section) => section !== identity);
  const logMin = Math.log10(minHz);
  const logMax = Math.log10(maxHz);
  return Array.from({ length: points }, (_, point) => {
    const hz = Math.pow(10, logMin + ((logMax - logMin) * point) / (points - 1));
    const linear = sections.reduce((total, section) => total * magnitude(section, hz, sampleRate), 1);
    return linear > 0 ? 20 * Math.log10(linear) + settings.preampDb : -maxBandGainDb;
  });
}
