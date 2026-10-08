import type { ReactNode } from "react";

import { isMac, isTauri } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import type { ForwardState, HostSnapshot, Settings, Stack } from "../lib/types";
import { AddMachineDialog } from "../machines/AddMachineDialog";
import { MachineRow } from "../machines/MachineRow";
import { availableUpdate, isUp, onlineCount, useStore } from "../state/store";
import type { DotState } from "../ui/Badges";
import { OsGlyph, StatusDot } from "../ui/Badges";
import { ActivityIcon, BookIcon, GearIcon, LifebuoyIcon, LogoMark, PlugIcon, PlusIcon, SpinnerIcon, StacksIcon } from "../ui/icons";
import { cx, navItemLook } from "../ui/primitives";

const VERSION = __APP_VERSION__;

/** The one way around the app: every page is a labeled item here, the
 * machines are listed under them, and Settings and Help sit at the bottom
 * with the version and, when there is one, the update. */
export function Sidebar() {
  const view = useStore((state) => state.view);
  const setView = useStore((state) => state.setView);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const selectMachine = useStore((state) => state.selectMachine);
  const update = useStore(availableUpdate);
  const openSettings = useStore((state) => state.openSettings);
  const addMachineOpen = useStore((state) => state.addMachineOpen);
  const setAddMachineOpen = useStore((state) => state.setAddMachineOpen);
  // Statuses, bridges, copies and the sharing snapshot change many times a
  // minute (five times a second during a copy). The sidebar shows only
  // counts and a sentence of them, so it selects those: a selector that
  // returns the same number or text as before does not redraw anything.
  const usesMachines = useStore((state) => state.settings?.use_machines ?? true);
  const computerOs = useStore((state) => state.computerInfo?.probe.os);
  const running = useStore((state) => state.stacks.filter((stack) => isUp(state.statuses[stack.id])).length);
  const ports = useStore((state) => portsOnLocalhost(state.stacks, state.forwards));
  const anyCopies = useStore((state) => Object.keys(state.copies).length > 0);
  const copiesRunning = useStore((state) => Object.values(state.copies).filter((copy) => !copy.finished_ms).length);
  const sharingDot = useStore((state) => sharingState(state.settings, state.host).dot);
  const sharingText = useStore((state) => sharingState(state.settings, state.host).text);

  const showActivity = anyCopies || view === "activity";
  // Room for the macOS traffic lights, which the overlay title bar draws over this corner.
  const leftPad = isTauri && isMac ? "pl-20" : "pl-4";

  return (
    <aside className="flex w-[232px] shrink-0 flex-col border-r border-line bg-surface/70">
      <div data-tauri-drag-region className={cx("flex h-16 shrink-0 items-center gap-2 pr-4", leftPad)}>
        <LogoMark size={18} className="text-accent" />
        <span data-tauri-drag-region className="text-[14px] font-semibold tracking-tight text-ink">
          dockerNanny
        </span>
      </div>

      <nav aria-label="Pages" className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-2 pb-3">
        <NavItem
          icon={<StacksIcon size={16} />}
          label="Stacks"
          count={running}
          countTitle="stacks running"
          active={view === "stacks" && selectedMachineId === null}
          onClick={() => selectMachine(null)}
        />
        {usesMachines ? (
          <NavItem
            icon={<PlugIcon size={16} />}
            label="Ports"
            count={ports}
            countTitle="ports on localhost"
            active={view === "ports"}
            onClick={() => setView("ports")}
          />
        ) : null}
        {showActivity ? (
          <NavItem
            icon={copiesRunning > 0 ? <SpinnerIcon size={16} className="text-accent" /> : <ActivityIcon size={16} />}
            label="Activity"
            count={copiesRunning}
            countTitle="copies running"
            active={view === "activity"}
            onClick={() => setView("activity")}
          />
        ) : null}
        <NavItem
          icon={<OsGlyph os={computerOs} size={16} />}
          label="This computer"
          title={sharingText}
          trailing={sharingDot ? <StatusDot state={sharingDot} label={sharingText} /> : null}
          active={view === "computer"}
          onClick={() => setView("computer")}
        />
        <MachinesSection usesMachines={usesMachines} onAdd={() => setAddMachineOpen(true)} />
      </nav>

      <footer className="shrink-0 space-y-0.5 border-t border-line p-2">
        {update ? (
          <button
            type="button"
            onClick={() => openSettings("updates")}
            className="mb-1.5 w-full rounded-lg border border-accent/40 bg-accent-soft px-3 py-2 text-left transition hover:border-accent"
          >
            <span className="block text-[12px] font-semibold text-accent">Version {update.version} is ready</span>
            <span className="block text-[11px] text-ink-2">See what's new and restart to update</span>
          </button>
        ) : null}
        <NavItem icon={<GearIcon size={16} />} label="Settings" active={view === "settings"} onClick={() => setView("settings")} />
        <NavItem icon={<LifebuoyIcon size={16} />} label="Help" active={view === "help"} onClick={() => setView("help")} />
        <div className="tabular px-2.5 pt-1.5 text-[11px] text-ink-3">Version {VERSION}</div>
      </footer>
      <AddMachineDialog open={addMachineOpen} onClose={() => setAddMachineOpen(false)} />
    </aside>
  );
}

/** The machines under the pages, with how many answer, and the two ways to get a new one. */
function MachinesSection({ usesMachines, onAdd }: { usesMachines: boolean; onAdd: () => void }) {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stacks = useStore((state) => state.stacks);
  const view = useStore((state) => state.view);
  const setView = useStore((state) => state.setView);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const selectMachine = useStore((state) => state.selectMachine);
  const openSettings = useStore((state) => state.openSettings);
  const machines = visibleMachines(allMachines, computerInfo);
  // Counted in the selector: every poll brings a new stats map, the count changes rarely.
  const online = useStore((state) => onlineCount(visibleMachines(state.machines, state.computerInfo), state.stats));

  if (!usesMachines) {
    return (
      <div className="mt-5 px-2.5 text-[12px] leading-relaxed text-ink-3">
        Using other machines is off.{" "}
        <button type="button" className="text-accent hover:underline" onClick={() => openSettings("general")}>
          Turn it on
        </button>
      </div>
    );
  }

  return (
    <div className="mt-5 space-y-0.5">
      <div className="flex items-baseline justify-between px-2.5 pb-1">
        <span className="text-[11px] font-semibold text-ink-3">Machines</span>
        {machines.length > 0 ? (
          <span className="tabular text-[11px] text-ink-3">
            {online} of {machines.length} online
          </span>
        ) : null}
      </div>
      {machines.map((machine) => (
        <MachineRow
          key={machine.id}
          machine={machine}
          stackCount={stacks.filter((stack) => stack.machine_id === machine.id).length}
          selected={view === "stacks" && selectedMachineId === machine.id}
          onSelect={() => selectMachine(machine.id)}
        />
      ))}
      {machines.length === 0 ? (
        <p className="px-2.5 pb-1 text-[12px] leading-relaxed text-ink-3">No machines yet. Add the one that will run your stacks.</p>
      ) : null}
      <NavItem icon={<PlusIcon size={16} />} label="Add machine" onClick={onAdd} active={false} quiet />
      <NavItem icon={<BookIcon size={16} />} label="How to prepare one" onClick={() => setView("guide")} active={view === "guide"} quiet />
    </div>
  );
}

function NavItem({
  icon,
  label,
  count,
  countTitle,
  trailing,
  title,
  active,
  quiet = false,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  count?: number;
  countTitle?: string;
  trailing?: ReactNode;
  title?: string;
  active: boolean;
  quiet?: boolean;
  onClick: () => void;
}) {
  const showCount = count !== undefined && count > 0;
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-current={active ? "page" : undefined}
      className={cx("flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition", navItemLook(active, quiet))}
    >
      <span className={cx("flex w-4 shrink-0 justify-center", active ? "text-accent" : "text-ink-3")}>{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {showCount ? (
        <span className="tabular text-[11px] text-ink-3" title={`${count} ${countTitle ?? ""}`.trim()}>
          {count}
        </span>
      ) : null}
      {trailing}
    </button>
  );
}

/** Every distinct localhost port a bridge that is up hands to a machine. */
function portsOnLocalhost(stacks: Stack[], forwards: Record<string, ForwardState>): number {
  const ports = new Set<number>();
  for (const stack of stacks) {
    const forward = forwards[stack.id];
    if (!forward?.up) continue;
    for (const port of forward.ports) ports.add(port.local);
  }
  return ports.size;
}

/** The one thing most worth knowing about sharing, as a dot and its sentence; the first match wins. */
function sharingState(settings: Settings | null, host: HostSnapshot | null): { dot: DotState | null; text: string } {
  const sharing = settings?.share_this_computer ?? false;
  if (!sharing) return { dot: null, text: "Sharing is off" };
  if (!host?.probed) return { dot: "pending", text: "Sharing: checking this computer" };
  if (host.pairing.armed) return { dot: "busy", text: `Pairing is on, code ${host.pairing.code}` };
  const missing = host.rows.filter((row) => row.state === "missing").length;
  if (missing > 0) return { dot: "attention", text: `Sharing: ${missing} to set up` };
  if (host.rows.some((row) => row.state === "restart")) return { dot: "attention", text: "Sharing: restart this computer once" };
  if (host.connected.length > 0) return { dot: "good", text: `Sharing: ${host.connected.length} connected` };
  return { dot: "good", text: "Sharing: ready" };
}
