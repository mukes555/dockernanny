import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine } from "../lib/types";
import { useStore } from "../state/store";
import { DropErrorLine, NewStackButton, useBrowse } from "../stacks/DropZone";
import { STACK_GRID, StackCard } from "../stacks/StackCard";
import { OsGlyph } from "../ui/Badges";
import { MachineIcon, SpinnerIcon, TerminalIcon } from "../ui/icons";
import { Menu, MenuItem, MenuSeparator } from "../ui/Menu";
import { Button, Chip, cx, EmptyPanel, Eyebrow } from "../ui/primitives";
import { Fact, ProbeFacts } from "../ui/ProbeFacts";
import { Term } from "../ui/Term";
import { DoctorRows } from "./DoctorRows";
import { MachineContainers } from "./MachineContainers";
import { TerminalDialog } from "./TerminalDialog";

/** One machine's own picture: how it is doing, whether everything it needs
 * is there, what runs on it, and the ways to use it. */
const NO_ROWS: never[] = [];

export function MachinePage({ machine }: { machine: Machine }) {
  const stats = useStore((state) => state.stats[machine.id]);
  // Selectors must return stable references: a fresh array per render would
  // re-render forever, so the filtering happens outside the selector.
  const allStacks = useStore((state) => state.stacks);
  const stacks = allStacks.filter((s) => s.machine_id === machine.id);
  const forwards = useStore((state) => state.forwards);
  const doctorRows = useStore((state) => state.doctor[machine.id] ?? NO_ROWS);
  const resetDoctor = useStore((state) => state.resetDoctor);
  const setMachines = useStore((state) => state.setMachines);
  const selectMachine = useStore((state) => state.selectMachine);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);
  const [terminal, setTerminal] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const browse = useBrowse();

  const online = stats?.online ?? false;
  const ports = stacks.reduce((count, stack) => count + (forwards[stack.id]?.up ? forwards[stack.id].ports.length : 0), 0);

  const check = async () => {
    setError(null);
    setChecking(true);
    resetDoctor(machine.id);
    try {
      await api.doctor(machine);
      setChecked(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setChecking(false);
    }
  };
  const refreshNumbers = () => void api.pollMachine(machine.id).catch((err) => setError(errorMessage(err)));
  const copyHere = () => setCopyOpen({ open: true, destinationMachineId: machine.id });

  // An action chosen in the rail's menu is carried out here, whether or not the page was open already.
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
      setMachines(await api.removeMachine(machine.id));
      // Its stacks went with it.
      useStore.getState().setStacks(await api.listStacks());
      selectMachine(null);
    } catch (err) {
      setError(errorMessage(err));
      setRemoving(false);
    }
  };

  return (
    <div className="mx-auto max-w-5xl space-y-5">
      <header className="rounded-2xl border border-line bg-surface p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <Eyebrow>Machine</Eyebrow>
            <div className="mt-1 flex items-center gap-2.5">
              <span className={cx("h-2.5 w-2.5 shrink-0 rounded-full", online ? "bg-good pulse" : "bg-hairline")} title={online ? "Online" : "Offline"} />
              <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-ink-2">
                <OsGlyph os={stats?.os} size={18} />
              </span>
              <h1 className="truncate text-xl font-semibold tracking-tight text-ink">{machine.name}</h1>
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
            </div>
            <div className="mono mt-1 text-[12px] text-ink-3" title={`${machine.user}@${machine.host}:${machine.port}`}>
              {machine.user}@{stats?.hostname || machine.host}:{machine.port}
            </div>
            {!online ? <div className="mt-1 text-[12px] text-ink-2">{stats?.error ?? "checking…"}</div> : null}
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button tone="primary" onClick={() => void check()} disabled={checking}>
              {checking ? <SpinnerIcon /> : null} {checked ? "Check again" : "Check connection"}
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
          </div>
        </div>

        {online && stats ? (
          <div className="mt-4">
            <ProbeFacts probe={stats}>
              <Fact label="From here">
                <span className="tabular">
                  {stacks.length} {stacks.length === 1 ? "stack" : "stacks"} · {ports} {ports === 1 ? "port" : "ports"} bridged to localhost
                </span>
              </Fact>
            </ProbeFacts>
          </div>
        ) : null}

        {doctorRows.length > 0 || checking ? (
          <div className="mt-4">
            <DoctorRows rows={doctorRows} checking={checking} />
          </div>
        ) : null}
        {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}

        {removing ? (
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-critical/40 bg-surface-2 px-3 py-2 text-[12px]">
            <span className="text-ink">
              Remove {machine.name} from this computer? {stacks.length === 1 ? "Its stack is forgotten here and its ports" : `Its ${stacks.length} stacks are forgotten here and their ports`} on localhost close. What runs on the machine stays as it is.
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
      </header>

      {stacks.length === 0 ? (
        <EmptyPanel
          icon={<MachineIcon size={26} />}
          title={`Nothing runs on ${machine.name} yet`}
          action={
            <div className="flex gap-2">
              <NewStackButton machine={machine} />
              <Button onClick={copyHere}>Copy a stack here</Button>
            </div>
          }
        >
          Pick or drop a compose file or a project folder to run it here, or bring a stack over from this computer or another machine.
        </EmptyPanel>
      ) : (
        <>
          <div className="flex items-end justify-between">
            <div>
              <Eyebrow>Stacks on {machine.name}</Eyebrow>
              <h2 className="mt-1 text-lg font-semibold tracking-tight">Running here, reachable on this computer</h2>
            </div>
            <div className="flex gap-2">
              <Button onClick={copyHere} title="Copy a stack from this computer or another machine to here">
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
      )}
      {online ? <MachineContainers machine={machine} /> : null}
      <TerminalDialog machine={terminal ? machine : null} onClose={() => setTerminal(false)} />
    </div>
  );
}
