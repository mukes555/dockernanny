import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Phase, ServiceState, Stack } from "../lib/types";
import { localPort } from "../lib/types";
import { isBusy, isUp, useStore } from "../state/store";
import { ExternalIcon, LogsIcon, PlayIcon, SpinnerIcon, StopIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { Button, Chip, cx } from "../ui/primitives";
import type { ChipTone } from "../ui/primitives";
import { BridgeControl } from "./Bridge";

/** Cards as wide as there is room for: one stack fills the width, and a
 * second sits beside it once both fit. */
export const STACK_GRID = "grid gap-4 grid-cols-[repeat(auto-fit,minmax(min(100%,26rem),1fr))]";

const PHASE_LABEL: Record<Phase, { text: string; tone: ChipTone }> = {
  idle: { text: "not started", tone: "neutral" },
  syncing: { text: "syncing", tone: "accent" },
  migrating: { text: "moving", tone: "accent" },
  starting: { text: "starting", tone: "accent" },
  waiting: { text: "getting ready", tone: "accent" },
  running: { text: "running", tone: "good" },
  partial: { text: "partly running", tone: "warning" },
  stopped: { text: "stopped", tone: "neutral" },
  stopping: { text: "stopping", tone: "accent" },
};

export function StackCard({ stack }: { stack: Stack }) {
  const status = useStore((state) => state.statuses[stack.id]);
  const forward = useStore((state) => state.forwards[stack.id]);
  const output = useStore((state) => state.output[stack.id]);
  const machine = useStore((state) => state.machines.find((m) => m.id === stack.machine_id));
  const setStacks = useStore((state) => state.setStacks);
  const openLogs = useStore((state) => state.openLogs);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const logsOpen = useStore((state) => state.logsFor === stack.id);
  const copy = useStore((state) => state.copies[stack.id]);
  const openProgress = useStore((state) => state.openProgress);
  const [expanded, setExpanded] = useState(false);
  const [removing, setRemoving] = useState<"ask" | "working" | null>(null);
  const [withVolumes, setWithVolumes] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const machineStats = useStore((state) => state.stats[stack.machine_id]);
  const removeStrip = useRef<HTMLDivElement>(null);

  const phase = status?.phase ?? "idle";
  const busy = isBusy(status);
  const lines = output ?? [];
  const running = isUp(status);
  // A copy cannot be interrupted halfway, so the backend refuses Stop while one runs.
  const copyRunning = copy !== undefined && !copy.finished_ms;
  const canStop = (busy || running) && !copyRunning;
  const spinning = busy || phase === "waiting";
  const ports = openablePorts(stack, status?.services ?? [], forward?.up ?? false);
  const hasContainers = (status?.services.length ?? 0) > 0;
  // Only a poll that came back without an answer means offline; no poll yet means not known.
  const machineOffline = machine !== undefined && machineStats !== undefined && !machineStats.online;
  const chip = machineOffline ? { text: "machine offline", tone: "neutral" as ChipTone } : PHASE_LABEL[phase];

  // The strip sits at the bottom of the card, which may be out of view when Remove is chosen.
  useEffect(() => {
    if (removing === "ask") removeStrip.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [removing]);

  // A new action clears the last one's error, so a failure does not outlive the next success.
  const call = (action: Promise<unknown>) => {
    setError(null);
    return action.catch((err) => setError(errorMessage(err)));
  };
  const toggleBridge = () => void call(api.setForwardPorts(stack.id, !stack.forward_ports).then(setStacks));
  const remove = async () => {
    setRemoving("working");
    try {
      setStacks(await api.removeStack(stack.id, withVolumes));
    } catch (err) {
      setError(errorMessage(err));
      setRemoving(null);
    }
  };

  return (
    <section className="flex flex-col rounded-2xl border border-line bg-surface p-4">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-[15px] font-semibold text-ink">{stack.name}</h2>
            <Chip tone={chip.tone}>
              {spinning && !machineOffline ? <SpinnerIcon size={10} /> : null}
              {chip.text}
            </Chip>
          </div>
          <div className="mt-0.5 truncate text-[11px] text-ink-3">
            on <span className="text-ink-2">{machine?.name ?? "a removed machine"}</span>
            <span className="mono"> · {stack.project_dir}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {/* The first thing wanted: a stopped stack starts, a running one opens. Rebuilding, the heaviest action, is in the menu. */}
          {!running ? (
            <Button
              size="sm"
              tone="primary"
              disabled={busy || !machine}
              onClick={() => void call(api.upStack(stack.id, false))}
              title="Sync the folder and start the stack on the machine; images are built only if missing"
            >
              <PlayIcon size={11} /> Start
            </Button>
          ) : ports.length > 0 ? (
            <Button size="sm" tone="primary" onClick={() => void api.openLocal(ports[0])} title={`Open http://localhost:${ports[0]} in the browser`}>
              <ExternalIcon size={11} /> Open
            </Button>
          ) : null}
          <Button
            size="sm"
            disabled={!canStop}
            onClick={() => void call(api.stopStack(stack.id))}
            title="Stop the containers; they stay, so Start brings them back quickly"
          >
            <StopIcon size={11} /> Stop
          </Button>
          <Button
            size="sm"
            tone={logsOpen ? "primary" : "secondary"}
            disabled={!machine}
            onClick={() => openLogs(logsOpen ? null : stack.id)}
            title="Follow the stack's logs"
          >
            <LogsIcon size={11} /> Logs
          </Button>
          {/* Always the same items, so none moves under the cursor when the state changes. */}
          <Menu label={`More actions for ${stack.name}`} width="w-56">
            <MenuItem onClick={() => void call(api.syncStack(stack.id))} disabled={busy || !machine}>
              Sync the folder now
            </MenuItem>
            <MenuItem onClick={() => void call(api.upStack(stack.id, true))} disabled={busy || !machine}>
              Rebuild images and restart
            </MenuItem>
            <MenuItem onClick={() => void call(api.restartStack(stack.id))} disabled={!running}>
              Restart containers
            </MenuItem>
            <MenuSeparator />
            <MenuItem onClick={() => setCopyOpen({ open: true, sourceStackId: stack.id })} disabled={busy || !machine}>
              Copy to…
            </MenuItem>
            <MenuItem onClick={toggleBridge} disabled={!machine && !stack.forward_ports}>
              {stack.forward_ports ? "Turn the bridge off" : "Turn the bridge on"}
            </MenuItem>
            <MenuItem onClick={() => void call(api.downStack(stack.id))} disabled={busy || !machine || !hasContainers}>
              Remove containers (keep data)
            </MenuItem>
            <MenuSeparator />
            <MenuItem onClick={() => setRemoving("ask")} danger>
              Remove stack…
            </MenuItem>
          </Menu>
        </div>
      </header>

      <div className={cx("mt-3 space-y-1", machineOffline && "opacity-60")}>
        {machineOffline ? <div className="text-[12px] text-ink-3">{machine?.name} does not answer right now; this is the last state seen.</div> : null}
        {status?.known && status.services.length === 0 && !busy ? <div className="text-[12px] text-ink-3">No containers on the machine yet.</div> : null}
        {!status?.known && !busy && !machineOffline ? <div className="text-[12px] text-ink-3">Waiting for the machine…</div> : null}
        {(status?.services ?? []).map((service) => (
          <ServiceRow key={service.service} stack={stack} service={service} forwardUp={forward?.up ?? false} />
        ))}
      </div>

      {lines.length > 0 && (busy || status?.error || expanded) ? (
        <div className="mt-3">
          <pre className="mono selectable max-h-64 overflow-auto rounded-lg bg-plane/60 px-3 py-2 text-[11px] leading-[1.5] text-ink-2">
            {(expanded ? lines.slice(-200) : lines.slice(-8)).map((line, index) => (
              <div key={index} className={cx("whitespace-pre-wrap break-all", line.stream === "stderr" && "text-ink")}>
                {line.text}
              </div>
            ))}
          </pre>
        </div>
      ) : null}

      <footer className="mt-3 flex items-center justify-between gap-3 text-[11px] text-ink-3">
        <div className="flex items-center gap-3">
          <SyncLine syncedAt={status?.synced_at_ms ?? null} files={status?.synced_files ?? 0} />
          {stack.live_sync ? <span className="text-accent">live</span> : null}
          {copy ? (
            <button
              type="button"
              className={cx("inline-flex items-center gap-1", copy.finished_ms ? (copy.failed ? "text-critical" : "hover:text-ink") : "text-accent")}
              onClick={() => openProgress(stack.id)}
              title="Show the copy's steps, bytes and result"
            >
              {!copy.finished_ms ? <SpinnerIcon size={10} /> : null}
              {copy.finished_ms ? (copy.failed ? "copy failed" : "last copy") : `copying from ${copy.from}`}
            </button>
          ) : null}
        </div>
        {lines.length > 0 ? (
          <button type="button" className="hover:text-ink" onClick={() => setExpanded((open) => !open)}>
            {expanded ? "hide output" : "show output"}
          </button>
        ) : null}
      </footer>
      {/* On a removed machine the switch stays while the bridge is on, so it can still be turned off. */}
      {machine || stack.forward_ports ? (
        <div className="mt-2 border-t border-line pt-2">
          <BridgeControl stack={stack} />
        </div>
      ) : null}
      {status?.error ? <div className="mt-2 text-[12px] text-critical">{status.error}</div> : null}
      {status?.sync_warning ? <div className="mt-2 text-[12px] text-warning">{status.sync_warning}</div> : null}
      {status?.folder_missing ? (
        <div className="mt-2 text-[12px] text-warning">The project folder is gone from the machine. Start copies it there again.</div>
      ) : null}
      {error ? <div className="mt-2 text-[12px] text-critical">{error}</div> : null}

      {removing ? (
        <div
          ref={removeStrip}
          className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-critical/40 bg-surface-2 px-3 py-2 text-[12px]"
        >
          {machine ? (
            <div>
              <div className="text-ink">Remove {stack.name}? It is stopped and its folder on the machine is deleted.</div>
              <label className="mt-1 flex items-center gap-2 text-ink-2">
                <input type="checkbox" checked={withVolumes} onChange={(e) => setWithVolumes(e.target.checked)} /> also delete its volumes (databases and other
                data)
              </label>
            </div>
          ) : (
            <div className="text-ink">Remove {stack.name}? Its machine is gone, so only this computer forgets it; nothing else is touched.</div>
          )}
          <div className="flex gap-2">
            <Button size="sm" tone="ghost" onClick={() => setRemoving(null)} disabled={removing === "working"}>
              Keep
            </Button>
            <Button size="sm" tone="danger" onClick={() => void remove()} busy={removing === "working"}>
              Remove
            </Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** The ports that open in a browser right now: TCP, of a running service,
 * through a bridge that is up; each once, as this computer numbers it. */
function openablePorts(stack: Stack, services: ServiceState[], forwardUp: boolean): number[] {
  if (!stack.forward_ports || !forwardUp) return [];
  const running = services.filter((service) => service.state === "running");
  const ports = running.flatMap((service) => service.ports.filter((port) => port.protocol === "tcp").map((port) => localPort(stack, port.published)));
  return [...new Set(ports)];
}

/** A service's dot and words, from Compose's own readiness (compose/ps.rs),
 * so the card and `docker compose up --wait` agree on what "ready" means. */
function serviceLook(service: ServiceState): { dot: string; detail: string; hint: string } {
  switch (service.readiness) {
    case "ready":
      return { dot: "bg-good", detail: service.health || service.state, hint: "Running, and ready" };
    case "starting":
      return { dot: "bg-accent", detail: service.state === "restarting" ? "restarting" : "starting", hint: "Running; its health check has not passed yet" };
    case "done":
      return { dot: "bg-hairline", detail: "job finished", hint: "A one-shot job other services waited for; it finished without an error" };
    case "unhealthy":
      return { dot: "bg-critical", detail: "unhealthy", hint: "Running, but its health check fails; its logs say why" };
    case "stopped": {
      const failed = service.state === "dead" || (service.state === "exited" && service.exit_code !== 0);
      const detail = service.state === "exited" ? `exited (${service.exit_code})` : service.state;
      return { dot: failed ? "bg-critical" : "bg-hairline", detail, hint: failed ? "Stopped with an error; its logs say why" : "Not running" };
    }
  }
}

function ServiceRow({ stack, service, forwardUp }: { stack: Stack; service: ServiceState; forwardUp: boolean }) {
  const running = service.state === "running";
  const look = serviceLook(service);
  return (
    <div className="flex items-center gap-2.5 rounded-lg px-2 py-1 text-[12px] hover:bg-surface-2/60">
      <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", look.dot)} />
      <span className="w-28 truncate font-medium text-ink">{service.service}</span>
      <span className="w-24 truncate text-ink-3" title={look.hint}>
        {look.detail}
      </span>
      <div className="flex flex-1 flex-wrap justify-end gap-1.5">
        {service.ports.map((port) => {
          const local = localPort(stack, port.published);
          const udp = port.protocol === "udp";
          const reachable = running && stack.forward_ports && forwardUp && !udp;
          const hint = udp
            ? "UDP does not go through the bridge"
            : !stack.forward_ports
              ? "The bridge is off for this stack"
              : !running
                ? "The service is not running"
                : !forwardUp
                  ? "The bridge is connecting…"
                  : `Open http://localhost:${local} (container port ${port.target})`;
          return (
            <button
              key={`${port.published}-${port.protocol}`}
              type="button"
              disabled={!reachable}
              onClick={() => void api.openLocal(local)}
              title={hint}
              className={cx(
                "mono inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] transition",
                reachable ? "border-good/40 text-good hover:bg-good/10" : "border-line text-ink-3",
              )}
            >
              {udp ? `${port.published}/udp` : `localhost:${local}`}
              {!udp && local !== port.published ? <span className="text-ink-3">({port.published})</span> : null}
              {reachable ? <ExternalIcon size={10} /> : null}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function SyncLine({ syncedAt, files }: { syncedAt: number | null; files: number }) {
  const now = useNow(5000);
  if (!syncedAt) return <span title="Sync times are kept while the app runs">not synced since the app started</span>;
  return (
    <span className="tabular">
      synced {relative(now - syncedAt)} · {files} files
    </span>
  );
}

function relative(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}

function useNow(everyMs: number): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}
