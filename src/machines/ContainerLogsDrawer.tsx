import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { api, errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";
import { XIcon } from "../ui/icons";
import { Button, cx } from "../ui/primitives";
import { useEscape } from "../ui/useEscape";

/** `docker logs -f` for one container on a machine, streamed over the same
 * ssh connection its Docker context uses, for exactly as long as this is open. */
export function ContainerLogsDrawer() {
  const target = useStore((state) => state.containerLogsFor);
  const lines = useStore((state) => state.containerLog);
  const openContainerLogs = useStore((state) => state.openContainerLogs);
  const [paused, setPaused] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useEscape(Boolean(target), () => openContainerLogs(null));

  useEffect(() => {
    if (!target) return;
    setPaused(false);
    setStartError(null);
    void api.startContainerLogs(target.machineId, target.id).catch((err) => setStartError(errorMessage(err)));
    return () => void api.stopContainerLogs(target.id).catch(console.warn);
  }, [target]);

  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current && !paused) el.scrollTop = el.scrollHeight;
  }, [lines.length, paused]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  return (
    <AnimatePresence>
      {target ? (
        <motion.aside
          className="fixed top-14 bottom-0 right-0 z-20 flex w-[560px] max-w-[80vw] flex-col border-l border-line bg-surface shadow-2xl"
          initial={{ x: 40, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: 40, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 34 }}
        >
          <header className="flex items-center gap-2 border-b border-line px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="text-[11px] uppercase tracking-[0.14em] text-ink-3">Container logs</div>
              <div className="truncate text-[14px] font-semibold text-ink">{target.name}</div>
            </div>
            <Button size="sm" onClick={() => setPaused((p) => !p)}>
              {paused ? "Follow" : "Pause"}
            </Button>
            <Button size="sm" tone="ghost" onClick={() => openContainerLogs(null)} aria-label="Close container logs">
              <XIcon />
            </Button>
          </header>
          <div ref={scroller} onScroll={onScroll} className="mono selectable min-h-0 flex-1 overflow-auto px-4 py-3 text-[12px] leading-[1.55]">
            {startError ? <div className="selectable text-critical">The logs could not start: {startError}</div> : null}
            {lines.length === 0 && !startError ? <div className="text-ink-3">waiting for output…</div> : null}
            {lines.map((line, index) => (
              <div key={index} className={cx("whitespace-pre-wrap break-all", line.stream === "stderr" ? "text-ink" : "text-ink-2")}>
                {line.text}
              </div>
            ))}
          </div>
        </motion.aside>
      ) : null}
    </AnimatePresence>
  );
}
