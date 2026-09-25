import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Phase, ServiceState, Stack } from "../lib/types";
import { localPort } from "../lib/types";
import { isBusy, useStore } from "../state/store";
import { ExternalIcon, LogsIcon, PlayIcon, RefreshIcon, SpinnerIcon, StopIcon } from "../ui/icons";
import { Menu, MenuItem } from "../ui/Menu";
import { Button, Chip, cx } from "../ui/primitives";
import type { ChipTone } from "../ui/primitives";
import { BridgeControl } from "./Bridge";

const PHASE_LABEL: Record<Phase, { text: string; tone: ChipTone }> = {
  idle: { text: "not started", tone: "neutral" },
  syncing: { text: "syncing", tone: "accent" },
  migrating: { text: "moving", tone: "accent" },
  starting: { text: "starting", tone: "accent" },
  running: { text: "running", tone: "good" },
  partial: { text: "partly running", tone: "warning" },
  stopped: { text: "stopped", tone: "neutral" },
  stopping: { text: "stopping", tone: "accent" },
  error: { text: "error", tone: "critical" },
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
  const canStop = busy || phase === "running" || phase === "partial";
  const primary = phase === "running" || phase === "partial" ? "Rebuild" : "Start";
  // Only a poll that came back without an answer means offline; no poll yet means not known.
  const machineOffline = machine !== undefined && machineStats !== undefined && !machineStats.online;
  const chip = machineOffline ? { text: "machine offline", tone: "neutral" as ChipTone } : PHASE_LABEL[phase];

  // The strip sits at the bottom of the card, which may be out of view when Remove is chosen.
  useEffect(() => {
    if (removing === "ask") removeStrip.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [removing]);

  const call = (action: Promise<unknown>) => action.catch((err) => setError(errorMessage(err)));
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
              {busy && !machineOffline ? <SpinnerIcon size={10} /> : null}
              {chip.text}
            </Chip>
          </div>
          <div className="mt-0.5 truncate text-[11px] text-ink-3">
            on <span className="text-ink-2">{machine?.name ?? "a removed machine"}</span>
            <span className="mono"> · {stack.project_dir}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            tone="primary"
            disabled={busy || !machine}
            onClick={() => void call(api.upStack(stack.id))}
            title={primary === "Start" ? "Sync the folder and start the stack on the machine" : "Sync the folder, rebuild what changed and restart what needs it"}
          >
            {primary === "Start" ? <PlayIcon size={11} /> : <RefreshIcon size={11} />} {primary}
          </Button>
          <Button size="sm" disabled={!canStop} onClick={() => void call(api.downStack(stack.id))} title="Stop and remove the containers; volumes and the folder stay">
            <StopIcon size={11} /> Stop
          </Button>
          <Button size="sm" tone={logsOpen ? "primary" : "secondary"} disabled={!machine} onClick={() => openLogs(logsOpen ? null : stack.id)} title="Follow the stack's logs">
            <LogsIcon size={11} /> Logs
          </Button>
          <Menu label={`More actions for ${stack.name}`} width="w-40">
            {/* Always there, so the items do not move under the cursor when the state changes. */}
            <MenuItem onClick={() => void call(api.restartStack(stack.id))} disabled={!(phase === "running" || phase === "partial")}>
              Restart
            </MenuItem>
            <MenuItem onClick={() => setCopyOpen({ open: true, sourceStackId: stack.id })} disabled={busy || !machine}>
              Copy to…
            </MenuItem>
            <MenuItem onClick={() => setRemoving("ask")} danger>
              Remove
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

      {lines.length > 0 && (busy || phase === "error" || expanded) ? (
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
          <button type="button" className="hover:text-ink disabled:opacity-40" disabled={busy || !machine} onClick={() => void call(api.syncStack(stack.id))} title="rsync the project folder again">
            re-sync
          </button>
          {copy ? (
            <button type="button" className={cx("inline-flex items-center gap-1", copy.finished_ms ? (copy.failed ? "text-critical" : "hover:text-ink") : "text-accent")} onClick={() => openProgress(stack.id)} title="Show the copy's steps, bytes and result">
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
      {status?.message ? <div className={cx("mt-2 text-[12px]", phase === "error" ? "text-critical" : "text-warning")}>{status.message}</div> : null}
      {error ? <div className="mt-2 text-[12px] text-critical">{error}</div> : null}

      {removing ? (
        <div ref={removeStrip} className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-critical/40 bg-surface-2 px-3 py-2 text-[12px]">
          {machine ? (
            <div>
              <div className="text-ink">Remove {stack.name}? It is stopped and its folder on the machine is deleted.</div>
              <label className="mt-1 flex items-center gap-2 text-ink-2">
                <input type="checkbox" checked={withVolumes} onChange={(e) => setWithVolumes(e.target.checked)} /> also delete its volumes (databases and other data)
              </label>
            </div>
          ) : (
            <div className="text-ink">Remove {stack.name}? Its machine is gone, so only this computer forgets it; nothing else is touched.</div>
          )}
          <div className="flex gap-2">
            <Button size="sm" tone="ghost" onClick={() => setRemoving(null)} disabled={removing === "working"}>
              Keep
            </Button>
            <Button size="sm" tone="danger" onClick={() => void remove()} disabled={removing === "working"}>
              {removing === "working" ? <SpinnerIcon size={11} /> : null} Remove
            </Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function ServiceRow({ stack, service, forwardUp }: { stack: Stack; service: ServiceState; forwardUp: boolean }) {
  const running = service.state === "running";
  const failed = service.state === "exited" && service.exit_code !== 0;
  const dot = running ? "bg-good" : failed ? "bg-critical" : service.state === "exited" ? "bg-hairline" : "bg-warning";
  const detail = service.health ? `${service.state}, ${service.health}` : service.state === "exited" ? `exited (${service.exit_code})` : service.state;
  return (
    <div className="flex items-center gap-2.5 rounded-lg px-2 py-1 text-[12px] hover:bg-surface-2/60">
      <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", dot)} />
      <span className="w-28 truncate font-medium text-ink">{service.service}</span>
      <span className="w-24 truncate text-ink-3">{detail}</span>
      <div className="flex flex-1 flex-wrap justify-end gap-1.5">
        {service.ports.map((port) => {
          const local = localPort(stack, port.published);
          const udp = port.protocol === "udp";
          const reachable = running && stack.forward_ports && forwardUp && !udp;
          const hint = udp ? "UDP does not go through the bridge" : !stack.forward_ports ? "The bridge is off for this stack" : !running ? "The service is not running" : !forwardUp ? "The bridge is connecting…" : `Open http://localhost:${local} (container port ${port.target})`;
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
