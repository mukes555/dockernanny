import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine } from "../lib/types";
import { useStore } from "../state/store";
import { DropErrorLine, NewStackButton, useBrowse } from "../stacks/DropZone";
import { STACK_GRID, StackCard } from "../stacks/StackCard";
import { OsGlyph } from "../ui/Badges";
import { MachineIcon, TerminalIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { Page, Tabs } from "../ui/Page";
import { Button, Card, Chip, cx, EmptyPanel } from "../ui/primitives";
import { Fact, ProbeFacts } from "../ui/ProbeFacts";
import { Term } from "../ui/Term";
import { DoctorRows } from "./DoctorRows";
import { MachineContainers } from "./MachineContainers";
import { TerminalDialog } from "./TerminalDialog";

const NO_ROWS: never[] = [];

type MachineTab = "stacks" | "containers" | "details";

/** One machine's own page: its stacks first, everything its Docker runs,
 * and its details with the result of the last connection check. */
export function MachinePage({ machine }: { machine: Machine }) {
  const stats = useStore((state) => state.stats[machine.id]);
  // Selectors must return stable references: a fresh array per render would
  // re-render forever, so the filtering happens outside the selector.
  const allStacks = useStore((state) => state.stacks);
  const stacks = allStacks.filter((s) => s.machine_id === machine.id);
  const doctorRows = useStore((state) => state.doctor[machine.id] ?? NO_ROWS);
  const resetDoctor = useStore((state) => state.resetDoctor);
  const removeMachine = useStore((state) => state.removeMachine);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const [tab, setTab] = useState<MachineTab>("stacks");
  const [checking, setChecking] = useState(false);
  const [terminal, setTerminal] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const browse = useBrowse();

  const online = stats?.online ?? false;

  // The check's rows live on the Details tab, so a check opens it.
  const check = async () => {
    setError(null);
    setTab("details");
    setChecking(true);
    resetDoctor(machine.id);
    try {
      await api.doctor(machine);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setChecking(false);
    }
  };
  const refreshNumbers = () => void api.pollMachine(machine.id).catch((err) => setError(errorMessage(err)));
  const copyHere = () => setCopyOpen({ open: true, destinationMachineId: machine.id });

  // An action chosen in the sidebar's menu is carried out here, whether or not the page was open already.
  const asked = useStore((state) => (state.machineAsk?.machineId === machine.id ? state.machineAsk.action : null));
  useEffect(() => {
    if (!asked) return;
    useStore.setState({ machineAsk: null });
    if (asked === "remove") setRemoving(true);
    if (asked === "terminal") setTerminal(true);
    if (asked === "check") void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asked]);

  const remove = async () => {
    try {
      await removeMachine(machine.id);
    } catch (err) {
      setError(errorMessage(err));
      setRemoving(false);
    }
  };

  const address = `${machine.user}@${stats?.hostname || machine.host}:${machine.port}`;
  const vitals = online && stats ? ` · load ${stats.load1.toFixed(1)}/${stats.cpus}` : "";

  return (
    <Page
      title={
        <>
          <span className={cx("h-2.5 w-2.5 shrink-0 rounded-full", online ? "bg-good pulse" : "bg-hairline")} title={online ? "Online" : "Offline"} />
          <OsGlyph os={stats?.os} size={18} className="shrink-0 text-ink-2" />
          <span className="truncate">{machine.name}</span>
          {machine.pinned ? (
            <Chip tone="good">
              <Term name="hostKey">host key pinned</Term>
            </Chip>
          ) : null}
          {machine.docker_context ? (
            <Chip tone="accent">
              <Term name="dockerContext">docker context</Term>
            </Chip>
          ) : null}
        </>
      }
      summary={
        <>
          <span className="mono">{address}</span>
          {vitals}
          {!online ? <span> · {stats?.error ?? "checking…"}</span> : null}
        </>
      }
      actions={
        <>
          <Button tone="primary" onClick={() => void check()} busy={checking}>
            Check connection
          </Button>
          <Button onClick={() => setTerminal(true)}>
            <TerminalIcon /> Terminal
          </Button>
          {/* Removing lives in here, away from the everyday buttons. */}
          <Menu label={`More for ${machine.name}`} width="w-48">
            <MenuItem onClick={() => void browse()}>New stack here…</MenuItem>
            <MenuItem onClick={copyHere}>Copy a stack here…</MenuItem>
            <MenuItem onClick={refreshNumbers}>Refresh its numbers</MenuItem>
            <MenuSeparator />
            <MenuItem onClick={() => setRemoving(true)} danger>
              Remove machine…
            </MenuItem>
          </Menu>
        </>
      }
      tabs={
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "stacks", label: "Stacks", count: stacks.length },
            { id: "containers", label: "Containers" },
            { id: "details", label: "Details" },
          ]}
        />
      }
    >
      {error ? <div className="text-[12px] text-critical">{error}</div> : null}
      {removing ? (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-critical/40 bg-surface-2 px-4 py-3 text-[12px]">
          <span className="text-ink">
            Remove {machine.name} from this computer?{" "}
            {stacks.length === 1 ? "Its stack is forgotten here and its ports" : `Its ${stacks.length} stacks are forgotten here and their ports`} on localhost
            close. What runs on the machine stays as it is.
          </span>
          <div className="flex gap-2">
            <Button size="sm" tone="ghost" onClick={() => setRemoving(false)}>
              Keep
            </Button>
            <Button size="sm" tone="danger" onClick={() => void remove()}>
              Remove
            </Button>
          </div>
        </div>
      ) : null}

      {tab === "stacks" ? <StacksTab machine={machine} onCopyHere={copyHere} /> : null}
      {tab === "containers" ? <MachineContainers machine={machine} /> : null}
      {tab === "details" ? (
        <>
          <Card title="This machine">
            {online && stats ? (
              <ProbeFacts probe={stats}>
                <Fact label="From here">
                  <BridgedPorts machine={machine} />
                </Fact>
              </ProbeFacts>
            ) : (
              <p className="text-[13px] text-ink-2">{stats?.error ?? "Waiting for the first answer…"}</p>
            )}
          </Card>
          <Card title="Connection check" description="What Check connection found: ssh, Docker, compose, rsync and the system.">
            {doctorRows.length > 0 || checking ? (
              <DoctorRows rows={doctorRows} checking={checking} />
            ) : (
              <p className="text-[13px] text-ink-2">Not checked since dockerNanny started. Check connection above runs it.</p>
            )}
          </Card>
        </>
      ) : null}
      <TerminalDialog machine={terminal ? machine : null} onClose={() => setTerminal(false)} />
    </Page>
  );
}

function StacksTab({ machine, onCopyHere }: { machine: Machine; onCopyHere: () => void }) {
  const allStacks = useStore((state) => state.stacks);
  const stacks = allStacks.filter((s) => s.machine_id === machine.id);
  if (stacks.length === 0) {
    return (
      <EmptyPanel
        icon={<MachineIcon size={26} />}
        title={`Nothing runs on ${machine.name} yet`}
        action={
          <div className="flex gap-2">
            <NewStackButton machine={machine} />
            <Button onClick={onCopyHere}>Copy a stack here</Button>
          </div>
        }
      >
        Pick or drop a compose file or a project folder to run it here, or bring a stack over from this computer or another machine.
      </EmptyPanel>
    );
  }
  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[13px] text-ink-2">Running on {machine.name}, reachable on this computer.</p>
        <div className="flex gap-2">
          <Button onClick={onCopyHere} title="Copy a stack from this computer or another machine to here">
            Copy a stack here
          </Button>
          <NewStackButton machine={machine} />
        </div>
      </div>
      <DropErrorLine />
      <div className={STACK_GRID}>
        {stacks.map((stack) => (
          <StackCard key={stack.id} stack={stack} />
        ))}
      </div>
    </>
  );
}

function BridgedPorts({ machine }: { machine: Machine }) {
  const allStacks = useStore((state) => state.stacks);
  const forwards = useStore((state) => state.forwards);
  const stacks = allStacks.filter((s) => s.machine_id === machine.id);
  const ports = stacks.reduce((count, stack) => count + (forwards[stack.id]?.up ? forwards[stack.id].ports.length : 0), 0);
  return (
    <span className="tabular">
      {stacks.length} {stacks.length === 1 ? "stack" : "stacks"} · {ports} {ports === 1 ? "port" : "ports"} bridged to localhost
    </span>
  );
}
