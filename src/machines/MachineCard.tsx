import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine, MachineStats } from "../lib/types";
import { useStore } from "../state/store";
import type { MachineAction } from "../state/store";
import { useBrowse } from "../stacks/DropZone";
import { BatteryPill, OsGlyph } from "../ui/Badges";
import { TerminalIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { gigabytes } from "../ui/Meter";
import { cx } from "../ui/primitives";

/** One machine in the rail: who it is, whether it answers, how it is doing.
 * Clicking it opens the machine's page. */
export function MachineCard({ machine, stats, stackCount, selected, onSelect }: { machine: Machine; stats?: MachineStats; stackCount: number; selected: boolean; onSelect: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const online = stats?.online ?? false;
  const hostname = stats?.hostname || machine.host;

  const browse = useBrowse();
  // Checking, the terminal and removing happen on the machine's page, which shows their results.
  const ask = (action: MachineAction) => useStore.getState().askMachine(machine.id, action);
  const newStackHere = () => {
    useStore.getState().selectMachine(machine.id);
    void browse();
  };
  const copyHere = () => useStore.getState().setCopyOpen({ open: true, destinationMachineId: machine.id });
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
      <Menu label={`${machine.name} menu`} className="absolute top-2 right-2" width="w-48">
        <MenuItem onClick={() => ask("check")}>Check connection</MenuItem>
        <MenuItem onClick={() => ask("terminal")}>Use from a terminal…</MenuItem>
        <MenuSeparator />
        <MenuItem onClick={newStackHere}>New stack here…</MenuItem>
        <MenuItem onClick={copyHere}>Copy a stack here…</MenuItem>
        <MenuItem onClick={refresh}>Refresh its numbers</MenuItem>
        <MenuSeparator />
        <MenuItem onClick={() => ask("remove")} danger>
          Remove…
        </MenuItem>
      </Menu>
      {error ? <div className="mt-2 text-[11px] text-critical">{error}</div> : null}
    </div>
  );
}

/** One line: enough to tell how the machine is doing at a glance. The
 * machine's page has the meters, the system and the rest. */
function LiveNumbers({ stats, stackCount }: { stats: MachineStats; stackCount: number }) {
  const hasMemory = stats.mem_total_mb > 0;
  const memoryShare = hasMemory ? Math.round((stats.mem_used_mb / stats.mem_total_mb) * 100) : 0;
  const noDocker = !stats.docker_version;
  // Each item stays whole; a narrow rail moves the last ones to a second line.
  return (
    <div className="mt-2 flex flex-wrap items-center gap-x-2.5 gap-y-1 whitespace-nowrap text-[11px] text-ink-3">
      <span className="tabular" title="Load over the last minute, and the CPUs">
        load {stats.load1.toFixed(1)}/{stats.cpus}
      </span>
      {hasMemory ? (
        <span className="tabular" title={`Memory in use: ${gigabytes(stats.mem_used_mb)} of ${gigabytes(stats.mem_total_mb)} GB`}>
          ram {memoryShare}%
        </span>
      ) : null}
      <BatteryPill battery={stats.battery} />
      {noDocker ? <span className="text-warning">no Docker</span> : null}
      <span className="tabular ml-auto">{stackCount === 1 ? "1 stack" : `${stackCount} stacks`}</span>
    </div>
  );
}
