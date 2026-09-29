import type { Battery } from "../lib/types";
import { cx } from "./primitives";

/** What a status dot says. Each has one colour everywhere in the app:
 * "attention" is something the user can fix or set up (amber), "failed"
 * something that broke (red). */
export type DotState = "good" | "busy" | "attention" | "failed" | "idle" | "pending";

// The text colour matches, so a pulsing dot's ring (currentColor) is its own colour.
const DOT_COLOUR: Record<DotState, string> = {
  good: "bg-good text-good",
  busy: "bg-accent text-accent",
  attention: "bg-warning text-warning",
  failed: "bg-critical text-critical",
  idle: "bg-hairline text-hairline",
  pending: "border border-ink-3 text-ink-3",
};

const DOT_SIZE = { sm: "h-1.5 w-1.5", md: "h-2 w-2", lg: "h-2.5 w-2.5" };

/** A coloured dot with its meaning for screen readers and on hover. `pulse`
 * for something live: a bridge carrying traffic, a computer connected now. */
export function StatusDot({
  state,
  label,
  pulse = false,
  size = "md",
  className,
}: {
  state: DotState;
  label: string;
  pulse?: boolean;
  size?: keyof typeof DOT_SIZE;
  className?: string;
}) {
  return (
    <span role="img" aria-label={label} title={label} className={cx("shrink-0 rounded-full", DOT_SIZE[size], DOT_COLOUR[state], pulse && "pulse", className)} />
  );
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
