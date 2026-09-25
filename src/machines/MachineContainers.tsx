import { useCallback, useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Container, ContainerAction, Machine } from "../lib/types";
import { useStore } from "../state/store";
import { LogsIcon, PlayIcon, RefreshIcon, SpinnerIcon, StopIcon } from "../ui/icons";
import { Button, Card, Chip, cx, Toggle } from "../ui/primitives";

const RUNNING_LIKE = ["running", "restarting"];

/** Everything the machine's Docker runs, seen and steered through its context
 * over ssh: the containers the compose-stack view leaves out, with the three
 * lifecycle verbs and live logs. The machine's own docker is the authority;
 * a failure here is the machine's answer, shown as it came. */
export function MachineContainers({ machine }: { machine: Machine }) {
  const online = useStore((state) => state.stats[machine.id]?.online ?? false);
  const openContainerLogs = useStore((state) => state.openContainerLogs);
  const pushNotice = useStore((state) => state.pushNotice);
  const [containers, setContainers] = useState<Container[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [onlyRunning, setOnlyRunning] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setContainers(await api.listContainers(machine.id));
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, [machine.id]);

  // Load when the machine is reachable, and keep it fresh; an offline machine
  // is not polled, so a machine that went to sleep is not hammered.
  useEffect(() => {
    if (!online) {
      setContainers(null);
      return;
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 8000);
    return () => window.clearInterval(timer);
  }, [online, refresh]);

  const act = async (container: Container, action: ContainerAction) => {
    setBusyId(container.id);
    try {
      await api.containerAction(machine.id, container.id, action);
      await refresh();
    } catch (err) {
      pushNotice(`Could not ${action} ${container.name}: ${errorMessage(err)}`);
    } finally {
      setBusyId(null);
    }
  };

  const shown = (containers ?? []).filter((c) => !onlyRunning || RUNNING_LIKE.includes(c.state)).sort(byRunningThenName);
  const runningCount = (containers ?? []).filter((c) => RUNNING_LIKE.includes(c.state)).length;

  return (
    <Card
      title="Containers on this machine"
      description="Everything the machine's Docker runs, over ssh, not only the stacks dockerNanny manages."
      actions={
        <>
          {containers && containers.length > 0 ? <Toggle checked={onlyRunning} onChange={setOnlyRunning} label="Only running" /> : null}
          <Button size="sm" tone="ghost" onClick={() => void refresh()} disabled={!online} aria-label="Refresh containers">
            <RefreshIcon />
          </Button>
        </>
      }
    >
      {!online ? (
        <p className="text-[13px] text-ink-2">The machine is offline. Its containers show here once it answers again.</p>
      ) : error ? (
        <div className="text-[13px] text-critical">{error}</div>
      ) : containers === null ? (
        <div className="flex items-center gap-2 text-[13px] text-ink-2">
          <SpinnerIcon /> reading the machine's Docker
        </div>
      ) : containers.length === 0 ? (
        <p className="text-[13px] text-ink-2">The machine's Docker has no containers.</p>
      ) : (
        <>
          <div className="mb-2 text-[11px] text-ink-3">
            {runningCount} of {containers.length} running
          </div>
          <div className="overflow-hidden rounded-xl border border-line">
            {shown.map((container, index) => (
              <ContainerRow key={container.id} container={container} busy={busyId === container.id} first={index === 0} onAct={(action) => void act(container, action)} onLogs={() => openContainerLogs({ machineId: machine.id, id: container.id, name: container.name })} />
            ))}
            {shown.length === 0 ? <div className="px-3 py-3 text-[12px] text-ink-3">Nothing is running. Turn off "Only running" to see the rest.</div> : null}
          </div>
        </>
      )}
    </Card>
  );
}

function ContainerRow({ container, busy, first, onAct, onLogs }: { container: Container; busy: boolean; first: boolean; onAct: (action: ContainerAction) => void; onLogs: () => void }) {
  const running = RUNNING_LIKE.includes(container.state);
  return (
    <div className={cx("flex items-center gap-3 px-3 py-2.5 text-[12px]", !first && "border-t border-line")}>
      <span className={cx("h-2 w-2 shrink-0 rounded-full", dotFor(container))} title={container.status} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-ink">{container.name}</span>
          {container.project ? <Chip tone="neutral">{container.project}</Chip> : null}
        </div>
        <div className="mono mt-0.5 truncate text-[11px] text-ink-3" title={`${container.image}${container.ports ? ` · ${container.ports}` : ""}`}>
          {container.image}
          {container.ports ? <span className="text-ink-3"> · {container.ports}</span> : null}
        </div>
      </div>
      <div className="hidden w-40 shrink-0 truncate text-[11px] text-ink-3 sm:block" title={container.status}>
        {container.status}
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {running ? (
          <>
            <Button size="sm" tone="ghost" disabled={busy} onClick={() => onAct("restart")} title="Restart" aria-label={`Restart ${container.name}`}>
              {busy ? <SpinnerIcon size={11} /> : <RefreshIcon size={11} />}
            </Button>
            <Button size="sm" tone="ghost" disabled={busy} onClick={() => onAct("stop")} title="Stop" aria-label={`Stop ${container.name}`}>
              <StopIcon size={11} />
            </Button>
          </>
        ) : (
          <Button size="sm" tone="ghost" disabled={busy} onClick={() => onAct("start")} title="Start" aria-label={`Start ${container.name}`}>
            {busy ? <SpinnerIcon size={11} /> : <PlayIcon size={11} />}
          </Button>
        )}
        <Button size="sm" tone="ghost" onClick={onLogs} title="Follow its logs" aria-label={`Logs of ${container.name}`}>
          <LogsIcon size={11} />
        </Button>
      </div>
    </div>
  );
}

const STATE_DOT: Record<string, string> = {
  running: "bg-good",
  restarting: "bg-warning pulse",
  paused: "bg-warning",
  created: "bg-hairline",
  exited: "bg-critical",
  dead: "bg-critical",
};

/** Red only for a failure: a container that finished with code 0 (a build
 * helper, a one-off task) just stopped. */
function dotFor(container: Container): string {
  const exitedCleanly = container.state === "exited" && /Exited \(0\)/.test(container.status);
  if (exitedCleanly) return "bg-hairline";
  return STATE_DOT[container.state] ?? "bg-hairline";
}

function byRunningThenName(a: Container, b: Container): number {
  const ar = RUNNING_LIKE.includes(a.state) ? 0 : 1;
  const br = RUNNING_LIKE.includes(b.state) ? 0 : 1;
  return ar - br || a.name.localeCompare(b.name);
}
