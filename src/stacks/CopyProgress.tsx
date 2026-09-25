import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import type { CopyProgress, CopyStep } from "../lib/types";
import { useStore } from "../state/store";
import { CheckIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { Button, cx } from "../ui/primitives";
import { useEscape } from "../ui/useEscape";

/** The side panel for one copy: every step and where it stands, the bytes
 * on their way with their speed, the last lines, and at the end where to
 * go and look at the result. Reopenable from the card while the copy is
 * remembered. */
export function CopyProgressDrawer() {
  const stackId = useStore((state) => state.progressFor);
  const progress = useStore((state) => (state.progressFor ? state.copies[state.progressFor] : undefined));
  const openProgress = useStore((state) => state.openProgress);
  const selectMachine = useStore((state) => state.selectMachine);
  const setView = useStore((state) => state.setView);
  const now = useNow(1000);
  useEscape(Boolean(stackId), () => openProgress(null));

  const openDestination = () => {
    if (!progress) return;
    if (progress.destination.kind === "machine") selectMachine(progress.destination.machine_id);
    else setView("computer");
    openProgress(null);
  };

  return (
    <AnimatePresence>
      {stackId && progress ? (
        <motion.aside
          className="fixed top-14 bottom-0 right-0 z-20 flex w-[520px] max-w-[80vw] flex-col border-l border-line bg-surface shadow-2xl"
          initial={{ x: 40, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: 40, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 34 }}
        >
          <header className="flex items-center gap-2 border-b border-line px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="text-[11px] uppercase tracking-[0.14em] text-ink-3">{progress.finished_ms ? (progress.failed ? "Copy failed" : "Copied") : "Copying"}</div>
              <div className="truncate text-[14px] font-semibold text-ink">
                {progress.name} <span className="font-normal text-ink-3">from</span> {progress.from} <span className="font-normal text-ink-3">to</span> {progress.to}
              </div>
            </div>
            <span className="tabular text-[12px] text-ink-3">{elapsedText((progress.finished_ms ?? now) - progress.started_ms)}</span>
            <Button size="sm" tone="ghost" onClick={() => openProgress(null)} aria-label="Close copy progress">
              <XIcon />
            </Button>
          </header>

          <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
            <ol className="space-y-1">
              {progress.steps.map((step) => (
                <StepRow key={step.name} step={step} />
              ))}
            </ol>
            {progress.current ? <TransferRow progress={progress} /> : null}
            {progress.lines.length > 0 ? (
              <pre className="mono selectable mt-4 max-h-56 overflow-auto rounded-lg bg-plane/60 px-3 py-2 text-[11px] leading-[1.5] text-ink-2">
                {progress.lines.map((line, index) => (
                  <div key={index} className="whitespace-pre-wrap break-all">
                    {line}
                  </div>
                ))}
              </pre>
            ) : null}
          </div>

          {progress.finished_ms ? (
            <footer className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
              <span className={cx("min-w-0 text-[12px]", progress.failed ? "text-critical" : "text-ink")}>{progress.outcome}</span>
              {!progress.failed ? (
                <Button tone="primary" size="sm" onClick={openDestination}>
                  Open on {progress.to}
                </Button>
              ) : null}
            </footer>
          ) : null}
        </motion.aside>
      ) : null}
    </AnimatePresence>
  );
}

function StepRow({ step }: { step: CopyStep }) {
  const mark = {
    pending: <span className="block h-2 w-2 rounded-full border border-ink-3" />,
    running: <SpinnerIcon size={12} className="text-accent" />,
    done: <CheckIcon size={12} className="text-good" />,
    failed: <XIcon size={12} className="text-critical" />,
    skipped: <span className="block h-0.5 w-2.5 rounded bg-ink-3" />,
  }[step.state];
  return (
    <li className={cx("flex items-center gap-2.5 rounded-lg px-2 py-1 text-[12px]", step.state === "running" ? "bg-accent-soft/60 text-ink" : step.state === "pending" ? "text-ink-3" : "text-ink-2", step.state === "failed" && "text-critical")}>
      <span className="flex w-3.5 shrink-0 items-center justify-center">{mark}</span>
      <span className={cx("truncate", step.state === "skipped" && "line-through")}>{step.name}</span>
    </li>
  );
}

/** The bar means "about": a volume travels compressed, so it may fill early. */
function TransferRow({ progress }: { progress: CopyProgress }) {
  const current = progress.current!;
  const percent = current.total_bytes ? Math.min(100, (current.bytes / current.total_bytes) * 100) : null;
  return (
    <div className="mt-4 rounded-xl border border-line bg-surface-2/50 px-3 py-2.5">
      <div className="flex items-center justify-between gap-3 text-[12px]">
        <span className="truncate text-ink">{current.label}</span>
        <span className="tabular shrink-0 text-ink-2">
          {megabytes(current.bytes)}
          {current.total_bytes ? ` of about ${megabytes(current.total_bytes)}` : ""}
          {current.per_second > 0 ? ` · ${megabytes(current.per_second)}/s` : ""}
        </span>
      </div>
      {/* Without a total the bar is indeterminate, which a progressbar says by leaving out the value. */}
      <div role="progressbar" aria-label={current.label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent === null ? undefined : Math.round(percent)} className="mt-2 h-1.5 overflow-hidden rounded-full bg-hairline">
        {percent !== null ? <div className="h-full rounded-full bg-accent transition-all duration-300" style={{ width: `${percent}%` }} /> : <div className="h-full w-1/3 animate-pulse rounded-full bg-accent/60" />}
      </div>
    </div>
  );
}

export function megabytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

function elapsedText(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
}

function useNow(everyMs: number): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}
