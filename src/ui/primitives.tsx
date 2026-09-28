// The handful of building blocks every screen is made of. Deep, not clever:
// one file, plain props, the same Tailwind vocabulary everywhere.

import { useState } from "react";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";

import { api } from "../lib/ipc";
import { SpinnerIcon } from "./icons";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

type Tone = "primary" | "secondary" | "ghost" | "danger";

const BUTTON: Record<Tone, string> = {
  primary: "bg-accent text-white hover:brightness-110",
  secondary: "border border-line bg-surface-2 text-ink-2 hover:text-ink",
  ghost: "text-ink-2 hover:bg-surface-2 hover:text-ink",
  danger: "border border-line text-ink-2 hover:border-critical hover:text-critical",
};

/** `busy` shows a spinner over the label instead of beside it: the button
 * keeps its exact size, so nothing around it jumps while it works. The label
 * is only faded out, so screen readers still hear what the button is. */
export function Button({
  tone = "secondary",
  size = "md",
  busy = false,
  className,
  children,
  disabled,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone; size?: "sm" | "md"; busy?: boolean }) {
  const pad = size === "sm" ? "px-2.5 py-1 text-[12px]" : "px-3.5 py-1.5 text-[13px]";
  const faded = disabled && !busy;
  return (
    <button disabled={disabled || busy} aria-busy={busy || undefined} className={cx("relative inline-flex items-center justify-center rounded-lg font-medium transition", BUTTON[tone], pad, faded && "opacity-40", busy && "cursor-wait", className)} {...rest}>
      <span className={cx("inline-flex items-center gap-1.5", busy && "opacity-0")}>{children}</span>
      {busy ? (
        <span className="absolute inset-0 flex items-center justify-center">
          <SpinnerIcon size={size === "sm" ? 12 : 14} />
        </span>
      ) : null}
    </button>
  );
}

export type ChipTone = "neutral" | "accent" | "good" | "warning" | "critical";

export function Chip({ tone = "neutral", children, className, title }: { tone?: ChipTone; children: ReactNode; className?: string; title?: string }) {
  const tones: Record<ChipTone, string> = {
    neutral: "border-line text-ink-3",
    accent: "border-accent bg-accent-soft text-accent",
    good: "border-good/40 text-good",
    warning: "border-warning/40 text-warning",
    critical: "border-critical/40 text-critical",
  };
  return (
    <span title={title} className={cx("inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px]", tones[tone], className)}>
      {children}
    </span>
  );
}

/** A titled block of a page. The title is a real heading, so a page reads
 * as sections instead of a column of look-alike boxes. */
export function Card({ title, description, actions, children, className }: { title?: ReactNode; description?: ReactNode; actions?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <section className={cx("rounded-xl border border-line bg-surface p-5", className)}>
      {title || actions ? (
        <header className="mb-4 flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            {title ? <h2 className="text-[14px] font-semibold text-ink">{title}</h2> : null}
            {description ? <p className="mt-1 text-[13px] leading-relaxed text-ink-2">{description}</p> : null}
          </div>
          {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      {children}
    </section>
  );
}

export const INPUT = "rounded-lg border border-line bg-surface-2 px-2.5 py-1.5 text-[13px] text-ink outline-none placeholder:text-ink-3 focus:border-accent focus:ring-2 focus:ring-accent/25";

/** Full width unless the caller gives a width: two width classes on one
 * element do not combine, and the stylesheet's order would pick the winner. */
function widthOf(className: string | undefined): string {
  const hasWidth = /(^|\s)w-/.test(className ?? "");
  return hasWidth ? "" : "w-full";
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="block">
      <span className="text-[12px] font-medium text-ink-2">{label}</span>
      <div className="mt-1.5">{children}</div>
      {hint ? <span className="mt-1 block text-[11px] text-ink-3">{hint}</span> : null}
    </label>
  );
}

export function TextInput({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cx(INPUT, widthOf(className), className)} {...rest} />;
}

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={cx(INPUT, widthOf(className), "py-1.5", className)} {...rest}>
      {children}
    </select>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (checked: boolean) => void; label?: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} className="inline-flex items-center gap-2 text-[13px] text-ink-2" onClick={() => onChange(!checked)}>
      <span className={cx("relative h-5 w-9 rounded-full transition", checked ? "bg-accent" : "bg-hairline")}>
        <span className={cx("absolute top-0.5 h-4 w-4 rounded-full bg-white transition", checked ? "left-4.5" : "left-0.5")} />
      </span>
      {label}
    </button>
  );
}

/** A small dashed note, for a narrow column or inside a card. */
export function EmptyState({ children, className }: { children: ReactNode; className?: string }) {
  return <p className={cx("rounded-xl border border-dashed border-hairline p-4 text-[13px] text-ink-2", className)}>{children}</p>;
}

/** A roomier empty state for the main area: an optional mark, a line, and a
 * way forward. Every "nothing here yet" screen looks the same this way. */
export function EmptyPanel({ icon, title, action, children, className }: { icon?: ReactNode; title: string; action?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <div className={cx("flex flex-col items-center gap-2 rounded-2xl border border-dashed border-hairline bg-surface/30 px-6 py-10 text-center", className)}>
      {icon ? <span className="mb-1 text-ink-3">{icon}</span> : null}
      <p className="text-[14px] font-semibold text-ink">{title}</p>
      {children ? <p className="max-w-md text-[13px] leading-relaxed text-ink-2">{children}</p> : null}
      {action ? <div className="mt-3">{action}</div> : null}
    </div>
  );
}

export function Eyebrow({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("text-[11px] uppercase tracking-[0.14em] text-ink-3", className)}>{children}</div>;
}

/** A command the user is meant to paste into a terminal, with a copy button
 * that shows on hover and on keyboard focus, and says whether it worked. */
export function CodeBlock({ code }: { code: string }) {
  const [copied, setCopied] = useState<"yes" | "failed" | null>(null);
  const copy = () => {
    api
      .copyText(code)
      .then(() => setCopied("yes"))
      .catch(() => setCopied("failed"))
      .finally(() => window.setTimeout(() => setCopied(null), 1500));
  };
  const label = copied === "yes" ? "copied" : copied === "failed" ? "select and copy by hand" : "copy";
  return (
    <div className="group relative rounded-lg border border-line bg-plane/60">
      <pre className="mono selectable overflow-x-auto px-3 py-2 text-[12px] leading-[1.6] text-ink-2">{code}</pre>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? label : "Copy the command"}
        className={cx(
          "absolute top-1.5 right-1.5 rounded-md border border-line bg-surface px-2 py-0.5 text-[11px] transition hover:text-ink focus-visible:opacity-100 group-hover:opacity-100",
          copied ? "opacity-100" : "opacity-0",
          copied === "yes" ? "text-good" : copied === "failed" ? "text-critical" : "text-ink-3",
        )}
      >
        {label}
      </button>
    </div>
  );
}
