import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";
import { Drawer } from "../ui/Drawer";
import { LogRow } from "../ui/LogRow";
import { Button, ErrorLine } from "../ui/primitives";
import { useFollowTail } from "../ui/useFollowTail";

/** `docker logs -f` for one container on a machine, streamed over the same
 * ssh connection its Docker context uses, for exactly as long as this is open. */
export function ContainerLogsDrawer() {
  const target = useStore((state) => state.containerLogsFor);
  const lines = useStore((state) => state.containerLog);
  const openContainerLogs = useStore((state) => state.openContainerLogs);
  const [paused, setPaused] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const { scroller, onScroll } = useFollowTail(lines.length, paused);

  // Another container starts afresh: following, no old error. Adjusted while
  // rendering, React's way for state that follows a prop, not in an effect.
  const [shownFor, setShownFor] = useState(target);
  if (shownFor !== target) {
    setShownFor(target);
    setPaused(false);
    setStartError(null);
  }

  // The stream lives exactly as long as the drawer shows this container.
  useEffect(() => {
    if (!target) return;
    let current = true;
    api.startContainerLogs(target.machineId, target.id).catch((err) => current && setStartError(errorMessage(err)));
    return () => {
      current = false;
      void api.stopContainerLogs(target.id).catch(console.warn);
    };
  }, [target]);

  return (
    <Drawer
      open={Boolean(target)}
      onClose={() => openContainerLogs(null)}
      eyebrow="Container logs"
      title={target?.name}
      closeLabel="Close container logs"
      controls={
        <Button size="sm" onClick={() => setPaused((p) => !p)}>
          {paused ? "Follow" : "Pause"}
        </Button>
      }
    >
      <div ref={scroller} onScroll={onScroll} className="mono selectable min-h-0 flex-1 overflow-auto px-4 py-3 text-[12px] leading-[1.55]">
        <ErrorLine error={startError ? `The logs could not start: ${startError}` : null} className="mt-0" />
        {lines.length === 0 && !startError ? <div className="text-ink-3">waiting for output…</div> : null}
        {lines.map((line) => (
          <LogRow key={line.seq} entry={line} />
        ))}
      </div>
    </Drawer>
  );
}
