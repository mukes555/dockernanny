import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { api, errorMessage } from "../lib/ipc";
import type { ServiceState } from "../lib/types";
import { parseLogLine } from "../lib/types";
import { useStore } from "../state/store";
import { XIcon } from "../ui/icons";
import { Button, cx, Select } from "../ui/primitives";
import { useEscape } from "../ui/useEscape";

// Selectors must return stable references; a fresh `[]` per render loops React.
const NO_SERVICES: ServiceState[] = [];

/** `compose logs -f` for one stack, streamed while the drawer is open. */
export function LogDrawer() {
  const stackId = useStore((state) => state.logsFor);
  const stack = useStore((state) => state.stacks.find((s) => s.id === state.logsFor));
  const services = useStore((state) => (state.logsFor ? state.statuses[state.logsFor]?.services : undefined) ?? NO_SERVICES);
  const lines = useStore((state) => state.logs);
  const openLogs = useStore((state) => state.openLogs);
  const clearLogs = useStore((state) => state.clearLogs);
  const [service, setService] = useState("");
  const [paused, setPaused] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useEscape(Boolean(stackId), () => openLogs(null));

  // The stream lives exactly as long as the drawer.
  useEffect(() => {
    if (!stackId) return;
    setService("");
    setPaused(false);
    setStartError(null);
    void api.startLogs(stackId).catch((err) => setStartError(errorMessage(err)));
    return () => void api.stopLogs(stackId).catch(console.warn);
  }, [stackId]);

  const shown = useMemo(() => {
    const parsed = lines.map((line) => ({ stream: line.stream, ...parseLogLine(line.text) }));
    if (!service || !stack) return parsed;
    // Compose prefixes a line with the container name, or with "service-1";
    // a custom container_name is matched exactly.
    const containerName = services.find((s) => s.service === service)?.container;
    const fromService = (head: string) => {
      const numbered = head.startsWith(`${service}-`) && /^\d+$/.test(head.slice(service.length + 1));
      return head === containerName || head.startsWith(`${stack.name}-${service}-`) || numbered;
    };
    return parsed.filter((line) => fromService(line.container));
  }, [lines, service, stack, services]);

  // Follow the tail unless the reader scrolled up to look at something.
  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current && !paused) el.scrollTop = el.scrollHeight;
  }, [shown.length, paused]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  return (
    <AnimatePresence>
      {stackId && stack ? (
        <motion.aside
          className="fixed top-14 bottom-0 right-0 z-20 flex w-[560px] max-w-[80vw] flex-col border-l border-line bg-surface shadow-2xl"
          initial={{ x: 40, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: 40, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 34 }}
        >
          <header className="flex items-center gap-2 border-b border-line px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="text-[11px] uppercase tracking-[0.14em] text-ink-3">Logs</div>
              <div className="truncate text-[14px] font-semibold text-ink">{stack.name}</div>
            </div>
            <Select value={service} onChange={(e) => setService(e.target.value)} className="w-36 shrink-0" aria-label="Service">
              <option value="">all services</option>
              {services.map((s) => (
                <option key={s.service} value={s.service}>
                  {s.service}
                </option>
              ))}
            </Select>
            <Button size="sm" onClick={() => setPaused((p) => !p)}>
              {paused ? "Follow" : "Pause"}
            </Button>
            <Button size="sm" tone="ghost" onClick={clearLogs}>
              Clear
            </Button>
            <Button size="sm" tone="ghost" onClick={() => openLogs(null)} aria-label="Close logs">
              <XIcon />
            </Button>
          </header>
          <div ref={scroller} onScroll={onScroll} className="mono selectable min-h-0 flex-1 overflow-auto px-4 py-3 text-[12px] leading-[1.55]">
            {startError ? <div className="selectable text-critical">The logs could not start: {startError}</div> : null}
            {shown.length === 0 && !startError ? <div className="text-ink-3">waiting for output…</div> : null}
            {shown.map((line, index) => (
              <div key={index} className="flex gap-2 whitespace-pre-wrap break-all">
                {line.container ? <span className="shrink-0 text-accent">{shortName(line.container, stack.name)}</span> : null}
                <span className={cx(line.stream === "stderr" ? "text-ink" : "text-ink-2")}>{line.text}</span>
              </div>
            ))}
          </div>
        </motion.aside>
      ) : null}
    </AnimatePresence>
  );
}

/** `shop-api-web-1` reads better as `web-1` next to the stack's own name. */
function shortName(container: string, stackName: string): string {
  const prefix = `${stackName}-`;
  return container.startsWith(prefix) ? container.slice(prefix.length) : container;
}
