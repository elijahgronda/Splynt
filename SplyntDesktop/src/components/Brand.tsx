/// The iOS app-icon squircle: a superellipse (n = 5), which is what gives Apple's
/// continuous corners their shape. A plain border-radius reads as a rounded
/// square next to the real icon.
const SQUIRCLE = (() => {
  const points: string[] = [];
  const steps = 96;
  for (let step = 0; step < steps; step += 1) {
    const angle = (step / steps) * Math.PI * 2;
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    const x = 50 + 50 * Math.sign(cos) * Math.abs(cos) ** (2 / 5);
    const y = 50 + 50 * Math.sign(sin) * Math.abs(sin) ** (2 / 5);
    points.push(`${x.toFixed(2)} ${y.toFixed(2)}`);
  }
  return `M${points.join("L")}Z`;
})();

/// Bar positions measured from assets/icon.png, in the icon's 100-unit square.
const BARS = [
  { x: 30.6, top: 47.6 },
  { x: 45.9, top: 30 },
  { x: 60.7, top: 40 },
];

export function BrandMark({ className }: { className?: string }) {
  return (
    <svg aria-hidden="true" className={className ? `brand__mark ${className}` : "brand__mark"} viewBox="0 0 100 100">
      <path d={SQUIRCLE} fill="var(--green)" />
      {BARS.map((bar) => <rect fill="#000" height={69.9 - bar.top} key={bar.x} rx={4.35} width={8.7} x={bar.x} y={bar.top} />)}
    </svg>
  );
}

export function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <div className={`brand ${compact ? "brand--compact" : ""}`} aria-label="Splynt">
      <BrandMark />
      {!compact && <span className="brand__name">Splynt</span>}
    </div>
  );
}
