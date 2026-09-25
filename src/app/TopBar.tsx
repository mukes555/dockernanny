import type { ReactNode } from "react";

import { isMac, isTauri } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import { onlineCount, useStore } from "../state/store";
import { ActivityIcon, GearIcon, HelpIcon, LogoMark, SpinnerIcon } from "../ui/icons";
import { cx } from "../ui/primitives";

const VERSION = __APP_VERSION__;

/** The overlay title bar: draggable everywhere that is not a control. */
export function TopBar() {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const machines = visibleMachines(allMachines, computerInfo);
  const stats = useStore((state) => state.stats);
  const stacks = useStore((state) => state.stacks);
  const statuses = useStore((state) => state.statuses);
  const forwards = useStore((state) => state.forwards);
  const portMapOpen = useStore((state) => state.portMapOpen);
  const setPortMapOpen = useStore((state) => state.setPortMapOpen);
  const copies = useStore((state) => state.copies);
  const activityOpen = useStore((state) => state.activityOpen);
  const setActivityOpen = useStore((state) => state.setActivityOpen);
  const view = useStore((state) => state.view);
  const setView = useStore((state) => state.setView);
  const selectMachine = useStore((state) => state.selectMachine);
  const update = useStore((state) => state.update);
  const setUpdateOpen = useStore((state) => state.setUpdateOpen);
  // Room for the macOS traffic lights when the native title bar is hidden.
  const leftPad = isTauri && isMac ? "pl-20" : "pl-5";

  const online = onlineCount(machines, stats);
  const running = stacks.filter((stack) => ["running", "partial"].includes(statuses[stack.id]?.phase ?? "")).length;
  const copyList = Object.values(copies);
  const copiesRunning = copyList.filter((c) => !c.finished_ms).length;
  const ports = new Set<number>();
  for (const stack of stacks) {
    const forward = forwards[stack.id];
    if (!forward?.up) continue;
    for (const port of forward.ports) ports.add(port.local);
  }

  return (
    <header data-tauri-drag-region className={`flex h-14 shrink-0 items-center justify-between border-b border-line bg-surface/70 ${leftPad} pr-5 backdrop-blur-md`}>
      <div data-tauri-drag-region className="flex items-center gap-2.5">
        <LogoMark className="text-accent" />
        <span className="text-[15px] font-semibold tracking-tight">dockerNanny</span>
        <span className="tabular text-[11px] text-ink-3">v{VERSION}</span>
        {update ? (
          <button type="button" onClick={() => setUpdateOpen(true)} className="rounded-full border border-accent bg-accent-soft px-2 py-0.5 text-[11px] font-medium text-accent transition hover:brightness-110" title="See what is new and install it">
            Update to {update.version}
          </button>
        ) : null}
      </div>
      <div data-tauri-drag-region className="flex items-center gap-6">
        <Stat value={`${online}/${machines.length}`} label="machines online" onClick={() => selectMachine(null)} title="Show every machine" />
        <Stat value={String(running)} label="stacks running" />
        <Stat value={String(ports.size)} label="ports on localhost" onClick={() => setPortMapOpen(!portMapOpen)} active={portMapOpen} title="Show the port map" />
        <div className="flex items-center gap-1">
          {copyList.length > 0 || activityOpen ? (
            <BarButton label="Activity" title="Background work: copies you can reopen" active={activityOpen} onClick={() => setActivityOpen(!activityOpen)}>
              {copiesRunning > 0 ? <SpinnerIcon size={16} className="text-accent" /> : <ActivityIcon size={16} />}
              {copiesRunning > 0 ? <span className="tabular absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[9px] font-semibold text-white">{copiesRunning}</span> : null}
            </BarButton>
          ) : null}
          <BarButton label="Help" title="Help, diagnostics and what the words mean" active={view === "help"} onClick={() => setView(view === "help" ? "stacks" : "help")}>
            <HelpIcon size={16} />
          </BarButton>
          <BarButton label="Settings" title="Settings" active={view === "settings"} onClick={() => setView(view === "settings" ? "stacks" : "settings")}>
            <GearIcon size={16} />
          </BarButton>
        </div>
      </div>
    </header>
  );
}

function BarButton({ label, title, active, onClick, children }: { label: string; title: string; active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      title={title}
      onClick={onClick}
      className={cx("relative rounded-lg p-2 text-ink-3 transition hover:bg-surface-2 hover:text-ink", active && "bg-accent-soft text-ink")}
    >
      {children}
    </button>
  );
}

function Stat({ value, label, onClick, active = false, title }: { value: string; label: string; onClick?: () => void; active?: boolean; title?: string }) {
  const body = (
    <>
      <span className="tabular text-[15px] font-semibold">{value}</span>
      <span className="text-[10px] uppercase tracking-[0.12em] text-ink-3">{label}</span>
    </>
  );
  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        title={title}
        className={`flex flex-col items-end rounded-lg px-2 py-1 leading-tight transition hover:bg-surface-2 ${active ? "bg-accent-soft" : ""}`}
      >
        {body}
      </button>
    );
  }
  return (
    <div data-tauri-drag-region className="flex flex-col items-end leading-tight">
      {body}
    </div>
  );
}
