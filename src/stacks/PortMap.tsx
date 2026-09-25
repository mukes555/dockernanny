import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine, Stack } from "../lib/types";
import { useStore } from "../state/store";
import { ExternalIcon, RefreshIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { Button, Chip, cx } from "../ui/primitives";
import { Term } from "../ui/Term";
import { useEscape } from "../ui/useEscape";
import type { Bridge, BridgeState } from "./Bridge";

/** One word per row; the chip's tooltip has the whole sentence with the time. */
const BRIDGE_WORD: Record<BridgeState, string> = { connected: "connected", connecting: "connecting", waiting: "waiting", off: "off" };
import { bridgeOf, useNow } from "./Bridge";

interface MappedPort {
  local: number;
  remote: number;
  stack: Stack;
  machine: Machine | undefined;
  service: string;
  bridge: Bridge;
}

/** Every localhost port this computer hands to another machine, in one
 * list, with the bridge's live state and its switch per stack. */
export function PortMap() {
  const open = useStore((state) => state.portMapOpen);
  const close = () => useStore.getState().setPortMapOpen(false);
  const allStacks = useStore((state) => state.stacks);
  const machines = useStore((state) => state.machines);
  const statuses = useStore((state) => state.statuses);
  const forwards = useStore((state) => state.forwards);
  const setStacks = useStore((state) => state.setStacks);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const view = useStore((state) => state.view);
  const now = useNow(1000);
  useEscape(open, close);
  const [working, setWorking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // On a machine's page the map shows that machine's ports, and says so;
  // everywhere else, and after "show all", every port the top bar counts.
  const [showAll, setShowAll] = useState(false);
  useEffect(() => {
    if (open) setShowAll(false);
  }, [open]);
  const filterMachine = view === "stacks" && !showAll ? machines.find((m) => m.id === selectedMachineId) : undefined;
  const stacks = filterMachine ? allStacks.filter((stack) => stack.machine_id === filterMachine.id) : allStacks;

  const rows: MappedPort[] = [];
  for (const stack of stacks) {
    const machine = machines.find((m) => m.id === stack.machine_id);
    const services = statuses[stack.id]?.services ?? [];
    const forward = forwards[stack.id];
    const bridge = bridgeOf(stack, statuses[stack.id], forward, now);
    const serviceFor = (published: number) => services.find((s) => s.ports.some((p) => p.published === published))?.service ?? "";
    if (forward && forward.ports.length > 0) {
      for (const port of forward.ports) {
        rows.push({ local: port.local, remote: port.remote, stack, machine, service: serviceFor(port.remote), bridge });
      }
      continue;
    }
    // Nothing forwarded yet: still show what the stack would publish.
    for (const service of services) {
      for (const port of service.ports.filter((p) => p.protocol === "tcp")) {
        rows.push({ local: stack.port_overrides[String(port.published)] ?? port.published, remote: port.published, stack, machine, service: service.service, bridge });
      }
    }
  }
  rows.sort((a, b) => a.local - b.local);

  const flip = async (stack: Stack) => {
    setWorking(stack.id);
    setError(null);
    try {
      setStacks(await api.setForwardPorts(stack.id, !stack.forward_ports));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(null);
    }
  };
  const restartAll = async () => {
    setWorking("all");
    setError(null);
    try {
      await api.resetForwards();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(null);
    }
  };

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="fixed top-16 right-5 z-30 w-[760px] max-w-[90vw] rounded-2xl border border-line bg-surface shadow-2xl"
          initial={{ y: -8, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: -8, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 32 }}
        >
          <header className="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <div className="text-[11px] uppercase tracking-[0.14em] text-ink-3">Port map</div>
              <div className="text-[14px] font-semibold text-ink">What localhost points at</div>
              {filterMachine ? (
                <div className="mt-0.5 text-[11px] text-ink-3">
                  Ports of {filterMachine.name} ·{" "}
                  <button type="button" className="text-accent hover:underline" onClick={() => setShowAll(true)}>
                    show all
                  </button>
                </div>
              ) : null}
            </div>
            <div className="flex items-center gap-1.5">
              <Button size="sm" onClick={() => void restartAll()} disabled={working !== null || rows.length === 0} title="Drop every bridge and let the running stacks bring theirs back">
                {working === "all" ? <SpinnerIcon size={11} /> : <RefreshIcon size={11} />} Restart all bridges
              </Button>
              <Button size="sm" tone="ghost" onClick={close} aria-label="Close port map">
                <XIcon />
              </Button>
            </div>
          </header>
          {error ? <div className="px-4 pt-3 text-[12px] text-critical">{error}</div> : null}
          {rows.length === 0 ? (
            <div className="px-4 py-6 text-[13px] text-ink-2">No ports yet. Start or copy a stack and its published ports appear here.</div>
          ) : (
            <table className="w-full text-[12px]">
              <thead className="whitespace-nowrap bg-surface-2 text-[10px] uppercase tracking-[0.12em] text-ink-3">
                <tr>
                  <th className="px-4 py-2 text-left font-medium">On this computer</th>
                  <th className="px-2 py-2 text-left font-medium">Goes to</th>
                  <th className="px-2 py-2 text-left font-medium">Stack</th>
                  <th className="px-2 py-2 text-left font-medium">
                    <Term name="bridge" />
                  </th>
                  <th className="px-4 py-2 text-right font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row, index) => {
                  const connected = row.bridge.state === "connected";
                  // The switch acts on the whole stack, so it sits on the stack's first row only.
                  const firstOfStack = index === 0 || rows[index - 1].stack.id !== row.stack.id;
                  return (
                    <tr key={`${row.stack.id}-${row.local}`} className="border-t border-line">
                      <td className="px-4 py-2">
                        <button
                          type="button"
                          className={cx("mono inline-flex items-center gap-1 text-ink hover:text-accent", !connected && "text-ink-3")}
                          disabled={!connected}
                          onClick={() => void api.openLocal(row.local)}
                          title={connected ? "Open in the browser" : "Not connected right now"}
                        >
                          localhost:{row.local}
                          {connected ? <ExternalIcon size={10} /> : null}
                        </button>
                      </td>
                      <td className="mono px-2 py-2 text-ink-2">
                        {row.machine?.name ?? "?"}:{row.remote}
                      </td>
                      <td className="px-2 py-2 text-ink-2">
                        {row.stack.name}
                        {row.service ? <span className="text-ink-3"> / {row.service}</span> : null}
                      </td>
                      <td className="px-2 py-2">
                        <Chip tone={row.bridge.tone} title={row.bridge.text} className="whitespace-nowrap">
                          {BRIDGE_WORD[row.bridge.state]}
                        </Chip>
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-right">
                        {firstOfStack ? (
                          <Button
                            size="sm"
                            tone="ghost"
                            onClick={() => void flip(row.stack)}
                            disabled={working !== null}
                            title={row.stack.forward_ports ? `All of ${row.stack.name}'s ports close here; the stack keeps running` : `Bring ${row.stack.name}'s ports to localhost`}
                          >
                            {working === row.stack.id ? <SpinnerIcon size={10} /> : null} {row.stack.forward_ports ? "Stop bridge" : "Start bridge"}
                          </Button>
                        ) : null}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
