import { api, errorMessage } from "../lib/ipc";
import type { Machine, MachineStats } from "../lib/types";
import { useStore } from "../state/store";
import type { MachineAction } from "../state/store";
import { useBrowse } from "../stacks/DropZone";
import { OsGlyph } from "../ui/Badges";
import { TerminalIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { cx } from "../ui/primitives";

/** One machine in the sidebar: whether it answers and one line on how it is
 * doing. Clicking it opens the machine's page; its menu shows on hover. */
export function MachineRow({
  machine,
  stats,
  stackCount,
  selected,
  onSelect,
}: {
  machine: Machine;
  stats?: MachineStats;
  stackCount: number;
  selected: boolean;
  onSelect: () => void;
}) {
  const online = stats?.online ?? false;
  const summary = summaryOf(stats, stackCount);
  const browse = useBrowse();
  // Checking, the terminal and removing happen on the machine's page, which shows their results.
  const ask = (action: MachineAction) => useStore.getState().askMachine(machine.id, action);
  const newStackHere = () => {
    useStore.getState().selectMachine(machine.id);
    void browse();
  };
  const copyHere = () => useStore.getState().setCopyOpen({ open: true, destinationMachineId: machine.id });
  const refresh = () => {
    void api.pollMachine(machine.id).catch((err) => useStore.getState().pushNotice(`Could not refresh ${machine.name}: ${errorMessage(err)}`));
  };

  return (
    <div className="group relative">
      <button
        type="button"
        onClick={onSelect}
        aria-current={selected ? "page" : undefined}
        title={`${machine.user}@${machine.host}:${machine.port}${stats?.error ? `\n${stats.error}` : ""}`}
        className={cx(
          "flex w-full items-center gap-2.5 rounded-lg py-1.5 pr-8 pl-2.5 text-left transition",
          selected ? "bg-accent-soft" : "hover:bg-surface-2",
        )}
      >
        <span className={cx("relative shrink-0", selected ? "text-ink" : "text-ink-3")}>
          <OsGlyph os={stats?.os} size={16} />
          <span className={cx("absolute -right-0.5 -bottom-0.5 h-2 w-2 rounded-full ring-2 ring-surface", online ? "bg-good" : "bg-hairline")} />
          <span className="sr-only">{online ? "online" : "offline"}</span>
        </span>
        <span className="min-w-0 flex-1">
          <span className={cx("flex items-center gap-1.5 text-[13px] font-medium", selected ? "text-ink" : "text-ink-2 group-hover:text-ink")}>
            <span className="truncate">{machine.name}</span>
            {machine.docker_context ? (
              <span className="shrink-0 text-accent" title="Docker context on">
                <TerminalIcon size={11} />
              </span>
            ) : null}
          </span>
          <span className={cx("tabular block truncate text-[11px]", summary.warn ? "text-warning" : "text-ink-3")}>{summary.text}</span>
        </span>
      </button>
      {/* Hidden until the row is hovered, the button has focus or its menu is open, so the list stays calm. */}
      <Menu
        label={`${machine.name} menu`}
        className="absolute top-2 right-1 opacity-0 transition group-hover:opacity-100 focus-within:opacity-100 has-[[aria-expanded=true]]:opacity-100"
        width="w-48"
      >
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
    </div>
  );
}

/** The line under the name: why it is not usable, or its stacks and load. */
function summaryOf(stats: MachineStats | undefined, stackCount: number): { text: string; warn: boolean } {
  if (!stats) return { text: "checking…", warn: false };
  if (!stats.online) return { text: "offline", warn: false };
  if (!stats.docker_version) return { text: "no Docker found", warn: true };
  const stacks = stackCount === 1 ? "1 stack" : `${stackCount} stacks`;
  return { text: `${stacks} · load ${stats.load1.toFixed(1)}/${stats.cpus}`, warn: false };
}
