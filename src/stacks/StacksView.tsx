import { visibleMachines } from "../lib/machines";
import type { Machine, Stack } from "../lib/types";
import { MachinePage } from "../machines/MachinePage";
import { useStore } from "../state/store";
import { ChevronRightIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { Button, cx, EmptyPanel } from "../ui/primitives";
import { DropErrorLine, DropHero, NewStackButton } from "./DropZone";
import { STACK_GRID, StackCard } from "./StackCard";

/** The main area of the "use other machines" role: one machine's page when
 * one is picked in the sidebar, otherwise every stack grouped by machine. */
export function StacksView() {
  const allStacks = useStore((state) => state.stacks);
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const statuses = useStore((state) => state.statuses);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const showSharing = useStore((state) => state.showSharing);
  // Records that point back at this computer are not machines; their stacks
  // stay out of sight too (Settings lists the records for removal).
  const machines = visibleMachines(allMachines, computerInfo);
  const hiddenIds = new Set(allMachines.filter((m) => !machines.includes(m)).map((m) => m.id));
  const stacks = allStacks.filter((s) => !hiddenIds.has(s.machine_id));
  const selected = machines.find((m) => m.id === selectedMachineId);
  // Keyed, so a confirm strip or a check in progress never carries over to another machine.
  if (selected) return <MachinePage key={selected.id} machine={selected} />;

  // With the role off the drop target would promise what the sidebar no longer
  // offers; stacks that still exist stay visible below as usual.
  const roleOff = settings !== null && !settings.use_machines;
  if (roleOff && stacks.length === 0) {
    return (
      <Page title="Stacks">
        <EmptyPanel
          className="mx-auto mt-6 max-w-xl"
          title="Using other machines is off"
          action={
            <div className="flex gap-2">
              <Button tone="primary" onClick={showSharing}>
                Open sharing
              </Button>
              <Button onClick={() => void saveSettings({ ...settings, use_machines: true })}>Turn it on</Button>
            </div>
          }
        >
          This computer is set up to be shared. Its page shows the sharing status and the pairing code.
        </EmptyPanel>
      </Page>
    );
  }

  const copyButton = (
    <Button
      onClick={() => setCopyOpen({ open: true })}
      disabled={machines.length === 0}
      title="Copy a stack's config and data between this computer and a machine, in either direction"
    >
      Copy a stack…
    </Button>
  );
  const running = stacks.filter((stack) => ["running", "partial"].includes(statuses[stack.id]?.phase ?? "")).length;
  const groups = machines.map((machine) => ({ machine, stacks: stacks.filter((s) => s.machine_id === machine.id) })).filter((group) => group.stacks.length > 0);
  const orphans = stacks.filter((s) => !allMachines.some((m) => m.id === s.machine_id));
  const summary =
    stacks.length === 0
      ? "Compose projects running on your machines, reachable here on localhost"
      : `${running} of ${stacks.length} running, on ${groups.length} ${groups.length === 1 ? "machine" : "machines"}`;

  return (
    <Page
      title="Stacks"
      summary={summary}
      actions={
        stacks.length > 0 ? (
          <>
            {copyButton}
            <NewStackButton />
          </>
        ) : null
      }
    >
      {stacks.length === 0 ? <DropHero extra={copyButton} /> : <DropErrorLine />}
      {groups.map((group) => (
        <MachineGroup key={group.machine.id} machine={group.machine} stacks={group.stacks} />
      ))}
      {orphans.length > 0 ? (
        <section className="space-y-3">
          <h2 className="text-[13px] font-semibold text-ink-2">On a removed machine</h2>
          <div className={STACK_GRID}>
            {orphans.map((stack) => (
              <StackCard key={stack.id} stack={stack} />
            ))}
          </div>
        </section>
      ) : null}
    </Page>
  );
}

/** One machine's stacks under a heading that opens the machine's page. */
function MachineGroup({ machine, stacks }: { machine: Machine; stacks: Stack[] }) {
  const online = useStore((state) => state.stats[machine.id]?.online ?? false);
  const selectMachine = useStore((state) => state.selectMachine);
  return (
    <section className="space-y-3">
      <button
        type="button"
        onClick={() => selectMachine(machine.id)}
        className="group inline-flex items-center gap-2 text-[13px] font-semibold text-ink"
        title={`Open ${machine.name}'s page`}
      >
        <span className={cx("h-2 w-2 rounded-full", online ? "bg-good" : "bg-hairline")} />
        <span className="group-hover:text-accent">{machine.name}</span>
        <span className="font-normal text-ink-3">
          · {stacks.length} {stacks.length === 1 ? "stack" : "stacks"}
        </span>
        <ChevronRightIcon size={13} className="text-ink-3 group-hover:text-accent" />
      </button>
      <div className={STACK_GRID}>
        {stacks.map((stack) => (
          <StackCard key={stack.id} stack={stack} />
        ))}
      </div>
    </section>
  );
}
