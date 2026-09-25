import { visibleMachines } from "../lib/machines";
import { useStore } from "../state/store";
import { BookIcon, PlusIcon } from "../ui/icons";
import { Button, cx, EmptyState, Eyebrow } from "../ui/primitives";
import { AddMachineDialog } from "./AddMachineDialog";
import { MachineCard } from "./MachineCard";
import { ThisComputerBlock } from "./ThisComputerBlock";

/** The left column: this computer on top, then every machine that can run a
 * stack, with live load. Picking a machine opens its page. */
export function MachineRail() {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stats = useStore((state) => state.stats);
  const stacks = useStore((state) => state.stacks);
  const settings = useStore((state) => state.settings);
  const view = useStore((state) => state.view);
  const setView = useStore((state) => state.setView);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const selectMachine = useStore((state) => state.selectMachine);
  const addMachineOpen = useStore((state) => state.addMachineOpen);
  const setAddMachineOpen = useStore((state) => state.setAddMachineOpen);
  const usesMachines = settings?.use_machines ?? true;
  const machines = visibleMachines(allMachines, computerInfo);
  const listedStacks = stacks.filter((s) => machines.some((m) => m.id === s.machine_id));
  const showingAll = view === "stacks" && selectedMachineId === null;

  return (
    <aside className="flex w-[264px] shrink-0 flex-col border-r border-line bg-surface/40 p-3">
      <ThisComputerBlock />
      <Eyebrow className="px-1 pb-2">Machines</Eyebrow>
      <div className="min-h-0 flex-1 space-y-2 overflow-auto">
        {!usesMachines ? <EmptyState>Using other machines is off. Turn it on in Settings when you want to send stacks elsewhere.</EmptyState> : null}
        {usesMachines && machines.length === 0 ? <EmptyState>No machines yet. Add the machine that will run your stacks.</EmptyState> : null}
        {usesMachines && machines.length > 0 ? (
          <button
            type="button"
            onClick={() => selectMachine(null)}
            className={cx("flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-[12px] transition", showingAll ? "bg-accent-soft text-ink" : "text-ink-2 hover:bg-surface-2 hover:text-ink")}
          >
            <span>All machines</span>
            <span className="tabular text-ink-3">{listedStacks.length === 1 ? "1 stack" : `${listedStacks.length} stacks`}</span>
          </button>
        ) : null}
        {usesMachines
          ? machines.map((machine) => (
              <MachineCard key={machine.id} machine={machine} stats={stats[machine.id]} stackCount={stacks.filter((s) => s.machine_id === machine.id).length} selected={view === "stacks" && selectedMachineId === machine.id} onSelect={() => selectMachine(machine.id)} />
            ))
          : null}
      </div>
      <div className="mt-auto space-y-1.5 border-t border-line pt-3">
        {usesMachines ? (
          <Button tone="primary" className="w-full" onClick={() => setAddMachineOpen(true)}>
            <PlusIcon /> Add machine
          </Button>
        ) : null}
        <button
          type="button"
          className={cx(
            "flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-[12px] transition",
            view === "guide" ? "bg-accent-soft text-ink" : "text-ink-3 hover:bg-surface-2 hover:text-ink",
          )}
          onClick={() => setView(view === "guide" ? "stacks" : "guide")}
        >
          <BookIcon /> Prepare another machine
        </button>
      </div>
      <AddMachineDialog open={addMachineOpen} onClose={() => setAddMachineOpen(false)} />
    </aside>
  );
}
