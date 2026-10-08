import { useCallback, useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Container, ContainerAction, Machine } from "../lib/types";
import { useStore } from "../state/store";
import type { DotState } from "../ui/Badges";
import { StatusDot } from "../ui/Badges";
import { RefreshIcon, SpinnerIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { Button, Card, Chip, cx, ErrorLine, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";

const RUNNING_LIKE = ["running", "restarting"];

/** Everything the machine's Docker runs, seen and steered through its context
 * over ssh: the containers the compose-stack view leaves out, with the three
 * lifecycle verbs and live logs. The machine's own docker is the authority;
 * a failure here is the machine's answer, shown as it came. */
export function MachineContainers({ machine }: { machine: Machine }) {
  const online = useStore((state) => state.stats[machine.id]?.online ?? false);
  const openContainerLogs = useStore((state) => state.openContainerLogs);
  const [loaded, setLoaded] = useState<Container[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [onlyRunning, setOnlyRunning] = useState(false);
  // A menu item has no room for a line, so a failed start or stop is a notice.
  const action = useAction("notice");
  const [busyId, setBusyId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoaded(await api.listContainers(machine.id));
      setLoadError(null);
    } catch (err) {
      setLoadError(errorMessage(err));
    }
  }, [machine.id]);

  // Load when the machine is reachable, and keep it fresh; an offline machine
  // is not polled, so a machine that went to sleep is not hammered. Nobody
  // reads the list while the window is hidden (in the tray, minimised), so
  // those polls are skipped and one read catches up when it shows again.
  useEffect(() => {
    if (!online) return;
    const refreshIfShown = () => {
      if (!document.hidden) void refresh();
    };
    const first = window.setTimeout(refreshIfShown, 0);
    const timer = window.setInterval(refreshIfShown, 8000);
    document.addEventListener("visibilitychange", refreshIfShown);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshIfShown);
    };
  }, [online, refresh]);

  const act = async (container: Container, verb: ContainerAction) => {
    setBusyId(container.id);
    await action.run(async () => {
      await api.containerAction(machine.id, container.id, verb);
      await refresh();
    }, `Could not ${verb} ${container.name}`);
  };

  // What was read while online is not shown once the machine stops answering.
  const containers = online ? loaded : null;

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
      <ContainerList
        online={online}
        loadError={loadError}
        containers={containers}
        onlyRunning={onlyRunning}
        busyId={action.busy ? busyId : null}
        onAct={(container, verb) => void act(container, verb)}
        onLogs={(container) => openContainerLogs({ machineId: machine.id, id: container.id, name: container.name })}
      />
    </Card>
  );
}

function ContainerList({
  online,
  loadError,
  containers,
  onlyRunning,
  busyId,
  onAct,
  onLogs,
}: {
  online: boolean;
  loadError: string | null;
  containers: Container[] | null;
  onlyRunning: boolean;
  busyId: string | null;
  onAct: (container: Container, verb: ContainerAction) => void;
  onLogs: (container: Container) => void;
}) {
  if (!online) return <p className="text-[13px] text-ink-2">The machine is offline. Its containers show here once it answers again.</p>;
  if (loadError) return <ErrorLine error={loadError} className="mt-0 text-[13px]" />;
  if (containers === null) {
    return (
      <div className="flex items-center gap-2 text-[13px] text-ink-2">
        <SpinnerIcon /> reading the machine's Docker
      </div>
    );
  }
  if (containers.length === 0) return <p className="text-[13px] text-ink-2">The machine's Docker has no containers.</p>;

  const shown = containers.filter((c) => !onlyRunning || RUNNING_LIKE.includes(c.state)).sort(byRunningThenName);
  const runningCount = containers.filter((c) => RUNNING_LIKE.includes(c.state)).length;
  return (
    <>
      <div className="mb-2 text-[11px] text-ink-3">
        {runningCount} of {containers.length} running
      </div>
      <div className="overflow-hidden rounded-xl border border-line">
        {shown.map((container, index) => (
          <ContainerRow
            key={container.id}
            container={container}
            busy={busyId === container.id}
            first={index === 0}
            onAct={(verb) => onAct(container, verb)}
            onLogs={() => onLogs(container)}
          />
        ))}
        {shown.length === 0 ? <div className="px-3 py-3 text-[12px] text-ink-3">Nothing is running. Turn off "Only running" to see the rest.</div> : null}
      </div>
    </>
  );
}

function ContainerRow({
  container,
  busy,
  first,
  onAct,
  onLogs,
}: {
  container: Container;
  busy: boolean;
  first: boolean;
  onAct: (verb: ContainerAction) => void;
  onLogs: () => void;
}) {
  const running = RUNNING_LIKE.includes(container.state);
  const dot = dotFor(container);
  return (
    <div className={cx("flex items-center gap-3 px-3 py-2.5 text-[12px]", !first && "border-t border-line")}>
      <StatusDot state={dot.state} pulse={dot.pulse} label={container.status} />
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
      {/* Words, not a row of look-alike icons: restarting a container and refreshing the list share one. */}
      <div className="flex shrink-0 items-center gap-1">
        {busy ? <SpinnerIcon size={11} /> : null}
        <Menu label={`Actions for ${container.name}`} width="w-40">
          <MenuItem onClick={onLogs}>Follow its logs</MenuItem>
          <MenuSeparator />
          {running ? (
            <>
              <MenuItem onClick={() => onAct("restart")} disabled={busy}>
                Restart
              </MenuItem>
              <MenuItem onClick={() => onAct("stop")} disabled={busy}>
                Stop
              </MenuItem>
            </>
          ) : (
            <MenuItem onClick={() => onAct("start")} disabled={busy}>
              Start
            </MenuItem>
          )}
        </Menu>
      </div>
    </div>
  );
}

const STATE_DOT: Record<string, { state: DotState; pulse: boolean }> = {
  running: { state: "good", pulse: false },
  restarting: { state: "attention", pulse: true },
  paused: { state: "attention", pulse: false },
  created: { state: "idle", pulse: false },
  exited: { state: "failed", pulse: false },
  dead: { state: "failed", pulse: false },
};

/** Red only for a failure: a container that finished with code 0 (a build
 * helper, a one-off task) just stopped. */
function dotFor(container: Container): { state: DotState; pulse: boolean } {
  const exitedCleanly = container.state === "exited" && /Exited \(0\)/.test(container.status);
  if (exitedCleanly) return { state: "idle", pulse: false };
  return STATE_DOT[container.state] ?? { state: "idle", pulse: false };
}

function byRunningThenName(a: Container, b: Container): number {
  const ar = RUNNING_LIKE.includes(a.state) ? 0 : 1;
  const br = RUNNING_LIKE.includes(b.state) ? 0 : 1;
  return ar - br || a.name.localeCompare(b.name);
}
