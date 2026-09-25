import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import type { CopyProgress } from "../lib/types";
import { useStore } from "../state/store";
import { CheckIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { Button, Chip } from "../ui/primitives";
import { useEscape } from "../ui/useEscape";

/** What is happening in the background, kept so it can be reopened after the
 * progress panel is closed: every copy this run has seen, running first. A
 * copy keeps going when its panel is closed; this is the way back to it. */
export function ActivityPanel() {
  const open = useStore((state) => state.activityOpen);
  const close = () => useStore.getState().setActivityOpen(false);
  const copies = useStore((state) => state.copies);
  const openProgress = useStore((state) => state.openProgress);
  const clearFinished = useStore((state) => state.clearFinishedCopies);
  const setActivityOpen = useStore((state) => state.setActivityOpen);
  const now = useNow(1000);
  useEscape(open, close);

  const list = Object.values(copies).sort((a, b) => Number(!!a.finished_ms) - Number(!!b.finished_ms) || b.started_ms - a.started_ms);
  const anyFinished = list.some((c) => c.finished_ms);

  const reopen = (copy: CopyProgress) => {
    openProgress(copy.stack_id);
    setActivityOpen(false);
  };

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="fixed top-16 right-5 z-30 w-[420px] max-w-[90vw] overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
          initial={{ y: -8, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: -8, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 32 }}
        >
          <header className="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <div className="text-[11px] uppercase tracking-[0.14em] text-ink-3">Activity</div>
              <div className="text-[14px] font-semibold text-ink">Background work</div>
            </div>
            <div className="flex items-center gap-1.5">
              {anyFinished ? (
                <Button size="sm" tone="ghost" onClick={clearFinished}>
                  Clear done
                </Button>
              ) : null}
              <Button size="sm" tone="ghost" onClick={close} aria-label="Close activity">
                <XIcon />
              </Button>
            </div>
          </header>
          {list.length === 0 ? (
            <div className="px-4 py-6 text-[13px] text-ink-2">Nothing is running. A copy you start shows here while it works, and stays so you can reopen it.</div>
          ) : (
            <ul className="max-h-[60vh] divide-y divide-line overflow-auto">
              {list.map((copy) => (
                <li key={copy.stack_id}>
                  <button type="button" onClick={() => reopen(copy)} className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition hover:bg-surface-2/60">
                    <StateMark copy={copy} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium text-ink">{copy.name}</div>
                      <div className="truncate text-[11px] text-ink-3">
                        {copy.from} to {copy.to}
                      </div>
                    </div>
                    <div className="shrink-0 text-right">
                      <StateChip copy={copy} />
                      <div className="tabular mt-0.5 text-[11px] text-ink-3">{elapsed((copy.finished_ms ?? now) - copy.started_ms)}</div>
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

function StateMark({ copy }: { copy: CopyProgress }) {
  if (!copy.finished_ms) return <SpinnerIcon size={14} className="text-accent" />;
  if (copy.failed) return <XIcon size={14} className="text-critical" />;
  return <CheckIcon size={14} className="text-good" />;
}

function StateChip({ copy }: { copy: CopyProgress }) {
  if (!copy.finished_ms) {
    const step = copy.steps.find((s) => s.state === "running");
    return <Chip tone="accent">{step ? step.name.split(" ")[0] : "working"}</Chip>;
  }
  return copy.failed ? <Chip tone="critical">failed</Chip> : <Chip tone="good">done</Chip>;
}

function elapsed(ms: number): string {
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
