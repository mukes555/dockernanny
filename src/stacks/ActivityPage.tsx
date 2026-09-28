import { useEffect, useState } from "react";

import type { CopyProgress } from "../lib/types";
import { useStore } from "../state/store";
import { CheckIcon, ChevronRightIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { Button, Chip, EmptyPanel } from "../ui/primitives";

/** What is happening in the background, kept so it can be reopened after the
 * progress panel is closed: every copy this run has seen, running first. A
 * copy keeps going when its panel is closed; this is the way back to it. */
export function ActivityPage() {
  const copies = useStore((state) => state.copies);
  const openProgress = useStore((state) => state.openProgress);
  const clearFinished = useStore((state) => state.clearFinishedCopies);
  const now = useNow(1000);

  const list = Object.values(copies).sort((a, b) => Number(!!a.finished_ms) - Number(!!b.finished_ms) || b.started_ms - a.started_ms);
  const running = list.filter((copy) => !copy.finished_ms).length;
  const anyFinished = list.some((copy) => copy.finished_ms);

  return (
    <Page
      title="Activity"
      summary={running > 0 ? `${running} ${running === 1 ? "copy" : "copies"} running` : "Copies started since dockerNanny opened"}
      actions={
        anyFinished ? (
          <Button tone="ghost" onClick={clearFinished}>
            Clear finished
          </Button>
        ) : null
      }
    >
      {list.length === 0 ? (
        <EmptyPanel title="Nothing is running">A copy you start shows here while it works, and stays so you can reopen its steps and result.</EmptyPanel>
      ) : (
        <ul className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
          {list.map((copy) => (
            <li key={copy.stack_id}>
              <button type="button" onClick={() => openProgress(copy.stack_id)} className="flex w-full items-center gap-3 px-4 py-3 text-left transition hover:bg-surface-2/60">
                <StateMark copy={copy} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium text-ink">{copy.name}</div>
                  <div className="truncate text-[12px] text-ink-3">
                    {copy.from} to {copy.to}
                  </div>
                </div>
                <div className="shrink-0 text-right">
                  <StateChip copy={copy} />
                  <div className="tabular mt-0.5 text-[11px] text-ink-3">{elapsed((copy.finished_ms ?? now) - copy.started_ms)}</div>
                </div>
                <ChevronRightIcon className="shrink-0 text-ink-3" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </Page>
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
