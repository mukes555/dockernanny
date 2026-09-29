/** A thin bar for load or memory: the accent colour up to 60 percent, amber to 85, red above. */
export function Meter({ label, value, text, size = "sm" }: { label: string; value: number; text: string; size?: "sm" | "md" }) {
  const tone = meterColour(value);
  const height = size === "md" ? "h-2" : "h-1.5";
  const font = size === "md" ? "text-[12px]" : "text-[11px]";
  const percent = Math.round(Math.min(100, Math.max(0, value)));
  return (
    <div className={`flex items-center gap-2 ${font}`}>
      <span className="w-8 uppercase tracking-wider text-ink-3">{label}</span>
      <span
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={text}
        className={`${height} flex-1 overflow-hidden rounded-full bg-hairline`}
      >
        <span className="block h-full rounded-full transition-all duration-700" style={{ width: `${Math.min(100, Math.max(0, value))}%`, background: tone }} />
      </span>
      <span className="tabular w-[84px] text-right text-ink-2">{text}</span>
    </div>
  );
}

function meterColour(percent: number): string {
  if (percent > 85) return "var(--critical)";
  if (percent > 60) return "var(--warning)";
  return "var(--accent)";
}

export function loadPercent(load1: number, cpus: number): number {
  return cpus > 0 ? Math.min(100, (load1 / cpus) * 100) : 0;
}

export function memoryPercent(usedMb: number, totalMb: number): number {
  return totalMb > 0 ? (usedMb / totalMb) * 100 : 0;
}
