import type { Battery } from "../lib/types";
import { cx } from "./primitives";

/** `up 9d 8h`, the way a person says it. */
export function uptimeText(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `up ${days}d ${hours}h`;
  if (hours > 0) return `up ${hours}h ${minutes}m`;
  return `up ${minutes}m`;
}

export function gigabytesOf(bytes: number): string {
  return `${(bytes / 1e9).toFixed(bytes >= 100e9 ? 0 : 1)} GB`;
}

/** Which family a probe's OS text belongs to, for the glyph. */
export function osFamily(os: string | null | undefined): "windows" | "macos" | "linux" | "unknown" {
  const text = (os ?? "").toLowerCase();
  if (text.includes("windows")) return "windows";
  if (text.includes("macos") || text.includes("darwin")) return "macos";
  if (text) return "linux";
  return "unknown";
}

/** A small mark for the operating system, drawn with strokes like the other icons. */
export function OsGlyph({ os, size = 16, className }: { os: string | null | undefined; size?: number; className?: string }) {
  const family = osFamily(os);
  const base = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.6,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  if (family === "windows") {
    return (
      <svg {...base} className={className} aria-label="Windows">
        <path d="M4 6l7-1v6H4zM13 4.5l7-1V11h-7zM4 13h7v6l-7-1zM13 13h7v7.5l-7-1z" />
      </svg>
    );
  }
  if (family === "macos") {
    return (
      <svg {...base} className={className} aria-label="macOS">
        <path d="M15.5 3.5c-1.6.2-3 1.4-3.3 2.9 1.5.1 3-1.1 3.3-2.9zM17.8 12.6c0-2 1.6-3 1.7-3.1-1-1.4-2.4-1.6-2.9-1.6-1.3-.1-2.4.7-3 .7-.7 0-1.6-.7-2.6-.7-1.4 0-2.6.8-3.3 2-1.4 2.4-.4 6 1 8 .7 1 1.5 2.1 2.5 2 1 0 1.4-.6 2.6-.6s1.6.6 2.6.6c1.1 0 1.8-1 2.5-2 .8-1.1 1.1-2.2 1.1-2.3-.1 0-2.2-.8-2.2-3z" />
      </svg>
    );
  }
  if (family === "linux") {
    return (
      <svg {...base} className={className} aria-label="Linux">
        <path d="M12 3c-2.2 0-3.5 1.8-3.5 4.2 0 1.6-.6 2.5-1.4 3.8-.9 1.5-1.6 3.1-1.6 4.5 0 2.4 3 4.5 6.5 4.5s6.5-2.1 6.5-4.5c0-1.4-.7-3-1.6-4.5-.8-1.3-1.4-2.2-1.4-3.8C15.5 4.8 14.2 3 12 3z" />
        <path d="M10 8.5h.01M14 8.5h.01M10.5 11.5c.5.6 2.5.6 3 0" />
      </svg>
    );
  }
  return (
    <svg {...base} className={className} aria-hidden>
      <rect x="4" y="5" width="16" height="11" rx="1.5" />
      <path d="M2 19h20" />
    </svg>
  );
}

/** Percent with a bolt while plugged in; red under 20 percent on battery. */
export function BatteryPill({ battery, className }: { battery: Battery | null | undefined; className?: string }) {
  if (!battery) return null;
  const low = !battery.charging && battery.percent < 20;
  return (
    <span
      className={cx(
        "inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10px] tabular",
        low ? "border-critical/40 text-critical" : "border-line text-ink-2",
        className,
      )}
      title={battery.charging ? "Plugged in" : "On battery"}
    >
      <svg width="14" height="10" viewBox="0 0 28 14" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
        <rect x="1" y="1.5" width="23" height="11" rx="2" />
        <rect x="25" y="4.5" width="2" height="5" rx="0.5" fill="currentColor" stroke="none" />
        <rect x="3" y="3.5" width={Math.max(1, 19 * (battery.percent / 100))} height="7" rx="1" fill="currentColor" stroke="none" />
      </svg>
      {battery.percent}%{battery.charging ? "⚡" : ""}
    </span>
  );
}
