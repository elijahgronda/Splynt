import { useEffect, useRef, useState } from "react";
import type { CSSProperties, InputHTMLAttributes } from "react";

type RangeStyle = CSSProperties & { "--range-progress": string };

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(maximum, value));
}

function rangeStyle(value: number, minimum: number, maximum: number): RangeStyle {
  const span = Math.max(0.0001, maximum - minimum);
  return { "--range-progress": `${clamp(((value - minimum) / span) * 100, 0, 100)}%` };
}

function clock(seconds: number) {
  const safe = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  return `${Math.floor(safe / 60)}:${String(safe % 60).padStart(2, "0")}`;
}

type SmoothRangeProps = Omit<InputHTMLAttributes<HTMLInputElement>, "max" | "min" | "onChange" | "style" | "type" | "value"> & {
  max: number;
  min?: number;
  onChange: (value: number) => void;
  value: number;
};

/**
 * A controlled slider whose fill is painted explicitly. WebKit's default
 * range track only paints grey, which made position and volume look frozen.
 */
export function SmoothRange({ max, min = 0, onChange, value, ...props }: SmoothRangeProps) {
  const [liveValue, setLiveValue] = useState(value);
  const interacting = useRef(false);

  useEffect(() => {
    if (!interacting.current) setLiveValue(value);
  }, [value]);

  return (
    <input
      {...props}
      max={max}
      min={min}
      onBlur={() => { interacting.current = false; }}
      onInput={(event) => {
        const next = Number(event.currentTarget.value);
        setLiveValue(next);
        onChange(next);
      }}
      onPointerDown={() => { interacting.current = true; }}
      onPointerUp={() => { interacting.current = false; }}
      style={rangeStyle(liveValue, min, max)}
      type="range"
      value={clamp(liveValue, min, max)}
    />
  );
}

type PlaybackProgressProps = {
  className?: string;
  disabled?: boolean;
  duration: number;
  isPlaying: boolean;
  onSeek: (value: number) => void;
  position: number;
};

/**
 * Audio elements publish `timeupdate` at a deliberately low cadence. This
 * small local clock interpolates between those authoritative positions so the
 * playhead looks continuous without re-rendering the complete desktop shell.
 */
export function PlaybackProgress({ className = "player-progress", disabled, duration, isPlaying, onSeek, position }: PlaybackProgressProps) {
  const maximum = Math.max(1, duration);
  const [livePosition, setLivePosition] = useState(() => clamp(position, 0, maximum));
  const scrubbing = useRef(false);

  useEffect(() => {
    if (scrubbing.current) return;
    setLivePosition(clamp(position, 0, maximum));
    if (!isPlaying || disabled) return;
    const anchorPosition = clamp(position, 0, maximum);
    const anchorTime = performance.now();
    let frame = 0;
    let lastPaint = anchorTime;
    const animate = (now: number) => {
      // 30fps is visually continuous for a thin rail and avoids turning the
      // player into a needless 60fps React workload.
      if (now - lastPaint >= 32 && !scrubbing.current) {
        lastPaint = now;
        setLivePosition(clamp(anchorPosition + (now - anchorTime) / 1000, 0, maximum));
      }
      frame = window.requestAnimationFrame(animate);
    };
    frame = window.requestAnimationFrame(animate);
    return () => window.cancelAnimationFrame(frame);
  }, [disabled, isPlaying, maximum, position]);

  return (
    <div className={className}>
      <span>{clock(livePosition)}</span>
      <input
        aria-label="Playback position"
        disabled={disabled}
        max={maximum}
        min={0}
        onBlur={() => { scrubbing.current = false; }}
        onInput={(event) => {
          const next = Number(event.currentTarget.value);
          setLivePosition(next);
          onSeek(next);
        }}
        onPointerDown={() => { scrubbing.current = true; }}
        onPointerUp={() => { scrubbing.current = false; }}
        step={0.05}
        style={rangeStyle(livePosition, 0, maximum)}
        type="range"
        value={clamp(livePosition, 0, maximum)}
      />
      <span>{clock(duration)}</span>
    </div>
  );
}
