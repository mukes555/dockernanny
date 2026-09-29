import type { ReactNode } from "react";

import { byteSize, stopwatch } from "../lib/format";
import type { CopyProgress, CopyStep } from "../lib/types";
import { useStore } from "../state/store";
import { Drawer } from "../ui/Drawer";
import { CheckIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { Button, cx } from "../ui/primitives";
import { useNow } from "../ui/useNow";

/** The side panel for one copy: every step and where it stands, the bytes
 * on their way with their speed, the last lines, and at the end where to
 * go and look at the result. Reopenable from the card while the copy is
 * remembered. */
export function CopyProgressDrawer() {
  const stackId = useStore((state) => state.progressFor);
  const progress = useStore((state) => (state.progressFor ? state.copies[state.progressFor] : undefined));
  const openProgress = useStore((state) => state.openProgress);
  const selectMachine = useStore((state) => state.selectMachine);
  const openComputer = useStore((state) => state.openComputer);
  const open = Boolean(stackId && progress);
  const running = open && !progress?.finished_ms;
  const now = useNow(1000, running);

  const openDestination = () => {
    if (!progress) return;
    if (progress.destination.kind === "machine") selectMachine(progress.destination.machine_id);
    else openComputer("docker");
    openProgress(null);
  };

  return (
    <Drawer
      open={open}
      onClose={() => openProgress(null)}
      eyebrow={progress ? stateWord(progress) : ""}
      closeLabel="Close copy progress"
      width={520}
      title={
        progress ? (
          <>
            {progress.name} <span className="font-normal text-ink-3">from</span> {progress.from} <span className="font-normal text-ink-3">to</span>{" "}
            {progress.to}
          </>
        ) : null
      }
      controls={progress ? <span className="tabular text-[12px] text-ink-3">{stopwatch((progress.finished_ms ?? now) - progress.started_ms)}</span> : null}
      footer={progress?.finished_ms ? <Outcome progress={progress} onOpen={openDestination} /> : null}
    >
      {progress ? (
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
      ) : null}
    </Drawer>
  );
}

function stateWord(progress: CopyProgress): string {
  if (!progress.finished_ms) return "Copying";
  return progress.failed ? "Copy failed" : "Copied";
}

function Outcome({ progress, onOpen }: { progress: CopyProgress; onOpen: () => void }) {
  return (
    <>
      <span className={cx("min-w-0 text-[12px]", progress.failed ? "text-critical" : "text-ink")}>{progress.outcome}</span>
      {!progress.failed ? (
        <Button tone="primary" size="sm" onClick={onOpen}>
          Open on {progress.to}
        </Button>
      ) : null}
    </>
  );
}

const STEP_MARK: Record<CopyStep["state"], ReactNode> = {
  pending: <span className="block h-2 w-2 rounded-full border border-ink-3" />,
  running: <SpinnerIcon size={12} className="text-accent" />,
  done: <CheckIcon size={12} className="text-good" />,
  failed: <XIcon size={12} className="text-critical" />,
  skipped: <span className="block h-0.5 w-2.5 rounded bg-ink-3" />,
};

const STEP_TEXT: Record<CopyStep["state"], string> = {
  pending: "text-ink-3",
  running: "bg-accent-soft/60 text-ink",
  done: "text-ink-2",
  failed: "text-critical",
  skipped: "text-ink-2",
};

function StepRow({ step }: { step: CopyStep }) {
  return (
    <li className={cx("flex items-center gap-2.5 rounded-lg px-2 py-1 text-[12px]", STEP_TEXT[step.state])}>
      <span className="flex w-3.5 shrink-0 items-center justify-center">{STEP_MARK[step.state]}</span>
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
          {byteSize(current.bytes)}
          {current.total_bytes ? ` of about ${byteSize(current.total_bytes)}` : ""}
          {current.per_second > 0 ? ` · ${byteSize(current.per_second)}/s` : ""}
        </span>
      </div>
      {/* Without a total the bar is indeterminate, which a progressbar says by leaving out the value. */}
      <div
        role="progressbar"
        aria-label={current.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent === null ? undefined : Math.round(percent)}
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-hairline"
      >
        {percent !== null ? (
          <div className="h-full rounded-full bg-accent transition-all duration-300" style={{ width: `${percent}%` }} />
        ) : (
          <div className="h-full w-1/3 animate-pulse rounded-full bg-accent/60" />
        )}
      </div>
    </div>
  );
}
