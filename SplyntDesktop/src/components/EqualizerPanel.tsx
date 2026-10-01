import { useId } from "react";
import { SmoothRange } from "./RangeSlider";
import {
  bandLabel,
  equalizerBands,
  equalizerPresets,
  flatEqualizer,
  formatDb,
  matchingPresetId,
  maxBandGainDb,
  maxPreampDb,
  responseCurveDb,
  suggestedPreampDb,
  type EqualizerSettings,
} from "../lib/equalizer";

/// The ten band sliders are native `input[type=range]` turned vertical in CSS
/// rather than the hand-built control iOS needs. Keyboard access, the arrow
/// keys, and the screen reader's own slider handling all come free that way,
/// and none of them are worth giving up for a custom thumb.
export function EqualizerPanel({ settings, onChange }: { settings: EqualizerSettings; onChange: (next: EqualizerSettings) => void }) {
  const curveId = useId();
  const activePreset = matchingPresetId(settings.gainsDb);
  const suggested = suggestedPreampDb(settings);
  const disabled = !settings.enabled;

  const setGain = (index: number, value: number) => {
    const gainsDb = settings.gainsDb.map((gain, position) => (position === index ? value : gain));
    onChange({ ...settings, gainsDb });
  };

  return (
    <section>
      <h2>Equalizer</h2>
      <label className="setting-row">
        <span><strong>Equalizer</strong><small>Applies to everything Splynt plays on this device, streamed or downloaded.</small></span>
        <input checked={settings.enabled} onChange={(event) => onChange({ ...settings, enabled: event.target.checked })} type="checkbox" />
      </label>

      <div aria-hidden="true" className={`equalizer-curve${disabled ? " equalizer-curve--off" : ""}`}>
        <svg preserveAspectRatio="none" viewBox="0 0 320 64">
          <line className="equalizer-curve__zero" x1="0" x2="320" y1="32" y2="32" />
          <polyline className="equalizer-curve__line" id={curveId} points={curvePoints(settings)} />
        </svg>
      </div>

      <div className={`equalizer-bands${disabled ? " equalizer-bands--off" : ""}`}>
        {equalizerBands.map((frequency, index) => (
          <div className="equalizer-band" key={frequency}>
            <input
              aria-label={`${frequency} hertz`}
              aria-valuetext={formatDb(settings.gainsDb[index] ?? 0)}
              className="equalizer-band__slider"
              disabled={disabled}
              max={maxBandGainDb}
              min={-maxBandGainDb}
              onChange={(event) => setGain(index, Number(event.target.value))}
              step={0.5}
              type="range"
              value={settings.gainsDb[index] ?? 0}
            />
            <span className="equalizer-band__label">{bandLabel(index)}</span>
          </div>
        ))}
      </div>

      <div className="setting-row setting-row--pickers">
        <span><strong>Preset</strong><small>{activePreset ? "A starting point you can still adjust." : "Custom curve."}</small></span>
        <span className="setting-row__controls">
          <select
            aria-label="Equalizer preset"
            disabled={disabled}
            onChange={(event) => {
              const preset = equalizerPresets.find((candidate) => candidate.id === event.target.value);
              if (preset) onChange({ ...settings, gainsDb: [...preset.gainsDb] });
            }}
            value={activePreset ?? "custom"}
          >
            {!activePreset && <option value="custom">Custom</option>}
            {equalizerPresets.map((preset) => <option key={preset.id} value={preset.id}>{preset.title}</option>)}
          </select>
        </span>
      </div>

      <label className="setting-row setting-row--slider">
        <span><strong>Preamp</strong><small>{preampHint(settings, suggested)}</small></span>
        <SmoothRange
          aria-label="Preamp"
          disabled={disabled}
          max={maxPreampDb}
          min={-maxPreampDb}
          onChange={(value) => onChange({ ...settings, preampDb: value })}
          step={0.5}
          value={settings.preampDb}
        />
      </label>

      <button disabled={disabled} onClick={() => onChange({ ...settings, gainsDb: [...flatEqualizer.gainsDb], preampDb: 0 })} type="button">
        Reset to flat
      </button>
    </section>
  );
}

/// Says what the boosted bands cost and what would pay for it, without moving
/// anything. A preamp that changed itself would fight the listener every time
/// they raised a band on purpose.
function preampHint(settings: EqualizerSettings, suggested: number): string {
  if (suggested === 0) return `${formatDb(settings.preampDb)} · cut this to make room for boosted bands.`;
  if (settings.preampDb <= suggested) return `${formatDb(settings.preampDb)} · boosted bands have room.`;
  return `${formatDb(settings.preampDb)} · boosted bands can clip loud tracks. ${formatDb(suggested)} would clear them.`;
}

function curvePoints(settings: EqualizerSettings): string {
  const curve = responseCurveDb(settings, { points: 64 });
  // The curve can exceed the band range once the preamp is in it, so the graph
  // is scaled to the widest it can get rather than clipping a peak flat against
  // the top edge.
  const range = maxBandGainDb + maxPreampDb;
  return curve
    .map((db, index) => {
      const x = (320 * index) / (curve.length - 1);
      const y = 32 - Math.max(-1, Math.min(1, db / range)) * 28;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
