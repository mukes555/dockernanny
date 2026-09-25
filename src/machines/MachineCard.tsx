import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine, MachineStats } from "../lib/types";
import { useStore } from "../state/store";
import { BatteryPill, OsGlyph, uptimeText } from "../ui/Badges";
import { TerminalIcon } from "../ui/icons";
import { Menu, MenuItem } from "../ui/Menu";
import { gigabytes, loadPercent, memoryPercent, Meter } from "../ui/Meter";
import { cx } from "../ui/primitives";

/** One machine in the rail: who it is, whether it answers, how it is doing.
 * Clicking it opens the machine's page. */
export function MachineCard({ machine, stats, stackCount, selected, onSelect }: { machine: Machine; stats?: MachineStats; stackCount: number; selected: boolean; onSelect: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const online = stats?.online ?? false;
  const hostname = stats?.hostname || machine.host;

  // Removing is asked on the machine's page, where the strip says what it does.
  const askRemove = () => useStore.getState().askRemoveMachine(machine.id);
  const refresh = () => {
    void api.pollMachine(machine.id).catch((err) => setError(errorMessage(err)));
  };

  return (
    <div className={cx("relative rounded-xl border p-3 transition", selected ? "border-accent bg-accent-soft" : "border-line bg-surface hover:border-ink-3/60")}>
      <button type="button" className="block w-full text-left" onClick={onSelect} title={`Show ${machine.name}`}>
        <div className="flex items-start gap-2.5 pr-6">
          <span className={cx("mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface-2", online ? "text-ink-2" : "text-ink-3")}>
            <OsGlyph os={stats?.os} size={16} />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className={cx("h-2 w-2 shrink-0 rounded-full", online ? "bg-good pulse" : "bg-hairline")} title={online ? "Online" : "Offline"} />
              <span className="truncate text-[13px] font-semibold text-ink">{machine.name}</span>
              {machine.docker_context ? (
                <span className="shrink-0 text-accent" title="Docker context on">
                  <TerminalIcon size={11} />
                  <span className="sr-only">Docker context on</span>
                </span>
              ) : null}
            </div>
            <div className="mono mt-0.5 truncate text-[11px] text-ink-3" title={`${machine.user}@${machine.host}:${machine.port}`}>
              {machine.user}@{hostname}
            </div>
          </div>
        </div>
        {online && stats ? <LiveNumbers stats={stats} stackCount={stackCount} /> : <div className="mt-2 text-[11px] text-ink-3">{stats?.error ?? "checking…"}</div>}
      </button>
      <Menu label={`${machine.name} menu`} className="absolute top-2 right-2">
        <MenuItem onClick={refresh}>Refresh its numbers</MenuItem>
        <MenuItem onClick={askRemove} danger>
          Remove…
        </MenuItem>
      </Menu>
      {error ? <div className="mt-2 text-[11px] text-critical">{error}</div> : null}
    </div>
  );
}

function LiveNumbers({ stats, stackCount }: { stats: MachineStats; stackCount: number }) {
  return (
    <div className="mt-2.5 space-y-1.5">
      <Meter label="load" value={loadPercent(stats.load1, stats.cpus)} text={`${stats.load1.toFixed(1)} / ${stats.cpus}`} />
      {stats.mem_total_mb > 0 ? <Meter label="ram" value={memoryPercent(stats.mem_used_mb, stats.mem_total_mb)} text={`${gigabytes(stats.mem_used_mb)} / ${gigabytes(stats.mem_total_mb)} GB`} /> : null}
      <div className="truncate text-[11px] text-ink-3" title={stats.os ?? undefined}>
        {stats.os ?? (stats.docker_version ? `Docker ${stats.docker_version}` : "Docker not found")}
      </div>
      <div className="flex items-center gap-2 text-[11px] text-ink-3">
        <BatteryPill battery={stats.battery} />
        {stats.uptime_s > 0 ? <span className="tabular">{uptimeText(stats.uptime_s)}</span> : null}
        <span className="tabular ml-auto">{stackCount === 1 ? "1 stack" : `${stackCount} stacks`}</span>
      </div>
    </div>
  );
}
