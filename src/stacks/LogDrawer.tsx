import { useEffect, useMemo, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { ServiceState } from "../lib/types";
import { useStore } from "../state/store";
import { Drawer } from "../ui/Drawer";
import { LogRow } from "../ui/LogRow";
import { Button, ErrorLine, Select } from "../ui/primitives";
import { useFollowTail } from "../ui/useFollowTail";

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

  // Another stack starts afresh: all services, following, no old error.
  // Adjusted while rendering, React's way for state that follows a prop.
  const [shownFor, setShownFor] = useState(stackId);
  if (shownFor !== stackId) {
    setShownFor(stackId);
    setService("");
    setPaused(false);
    setStartError(null);
  }

  // The stream lives exactly as long as the drawer shows this stack.
  useEffect(() => {
    if (!stackId) return;
    let current = true;
    api.startLogs(stackId).catch((err) => current && setStartError(errorMessage(err)));
    return () => {
      current = false;
      void api.stopLogs(stackId).catch(console.warn);
    };
  }, [stackId]);

  const shown = useMemo(() => {
    if (!service || !stack) return lines;
    // Compose prefixes a line with the container name, or with "service-1";
    // a custom container_name is matched exactly.
    const containerName = services.find((s) => s.service === service)?.container;
    const fromService = (head: string) => {
      const numbered = head.startsWith(`${service}-`) && /^\d+$/.test(head.slice(service.length + 1));
      return head === containerName || head.startsWith(`${stack.name}-${service}-`) || numbered;
    };
    return lines.filter((line) => fromService(line.container));
  }, [lines, service, stack, services]);
  const lastShownSeq = shown.length > 0 ? shown[shown.length - 1].seq : 0;
  const { scroller, onScroll } = useFollowTail(lastShownSeq, paused);

  return (
    <Drawer
      open={Boolean(stackId && stack)}
      onClose={() => openLogs(null)}
      eyebrow="Logs"
      title={stack?.name}
      closeLabel="Close logs"
      controls={
        <>
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
        </>
      }
    >
      <div ref={scroller} onScroll={onScroll} className="mono selectable min-h-0 flex-1 overflow-auto px-4 py-3 text-[12px] leading-[1.55]">
        <ErrorLine error={startError ? `The logs could not start: ${startError}` : null} className="mt-0" />
        {shown.length === 0 && !startError ? <div className="text-ink-3">waiting for output…</div> : null}
        {shown.map((line) => (
          <LogRow key={line.seq} entry={line} label={line.container && stack ? shortName(line.container, stack.name) : undefined} />
        ))}
      </div>
    </Drawer>
  );
}

/** `shop-api-web-1` reads better as `web-1` next to the stack's own name. */
function shortName(container: string, stackName: string): string {
  const prefix = `${stackName}-`;
  return container.startsWith(prefix) ? container.slice(prefix.length) : container;
}
