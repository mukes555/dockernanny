import { visibleMachines } from "../lib/machines";
import { MachinePage } from "../machines/MachinePage";
import { useStore } from "../state/store";
import { Button, EmptyPanel, Eyebrow } from "../ui/primitives";
import { DropHero, DropStrip } from "./DropZone";
import { StackCard } from "./StackCard";

/** The main area of the "use other machines" role: one machine's page when
 * one is picked in the rail, otherwise every stack grouped by machine. */
export function StacksView() {
  const allStacks = useStore((state) => state.stacks);
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
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

  // With the role off the drop target would promise what the rail no longer
  // offers; stacks that still exist stay visible below as usual.
  const roleOff = settings !== null && !settings.use_machines;
  if (roleOff && stacks.length === 0) {
    return (
      <EmptyPanel
        className="mx-auto mt-10 max-w-xl"
        title="Using other machines is off"
        action={
          <div className="flex gap-2">
            <Button tone="primary" onClick={showSharing}>
              Open this computer
            </Button>
            <Button onClick={() => void saveSettings({ ...settings, use_machines: true })}>Turn it on</Button>
          </div>
        }
      >
        This computer is set up to be shared. Its page shows the sharing status and the pairing code.
      </EmptyPanel>
    );
  }

  const copyButton = (
    <Button onClick={() => setCopyOpen({ open: true })} disabled={machines.length === 0} title="Copy a stack's config and data between this computer and a machine, in either direction">
      Copy a stack…
    </Button>
  );
  if (stacks.length === 0) return <DropHero extra={copyButton} />;

  const groups = machines.map((machine) => ({ machine, stacks: stacks.filter((s) => s.machine_id === machine.id) })).filter((group) => group.stacks.length > 0);
  const orphans = stacks.filter((s) => !allMachines.some((m) => m.id === s.machine_id));

  return (
    <div className="mx-auto max-w-5xl space-y-4">
      <div className="flex items-end justify-between">
        <div>
          <Eyebrow>All machines</Eyebrow>
          <h1 className="mt-1 text-lg font-semibold tracking-tight">Running elsewhere, reachable here</h1>
        </div>
        {copyButton}
      </div>
      <DropStrip />
      {groups.map((group) => (
        <section key={group.machine.id} className="space-y-3">
          <Eyebrow className="pt-2">
            on {group.machine.name} · {group.stacks.length} {group.stacks.length === 1 ? "stack" : "stacks"}
          </Eyebrow>
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            {group.stacks.map((stack) => (
              <StackCard key={stack.id} stack={stack} />
            ))}
          </div>
        </section>
      ))}
      {orphans.length > 0 ? (
        <section className="space-y-3">
          <Eyebrow className="pt-2">on a removed machine</Eyebrow>
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            {orphans.map((stack) => (
              <StackCard key={stack.id} stack={stack} />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}
