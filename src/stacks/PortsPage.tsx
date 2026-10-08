import { useState } from "react";

import { plural } from "../lib/format";
import { api } from "../lib/ipc";
import type { Machine, Stack } from "../lib/types";
import { useStore } from "../state/store";
import { ExternalIcon, RefreshIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { Button, Chip, cx, EmptyPanel } from "../ui/primitives";
import { Term } from "../ui/Term";
import { useAction } from "../ui/useAction";
import { useNow } from "../ui/useNow";
import type { Bridge, BridgeState } from "./Bridge";
import { bridgeOf } from "./Bridge";

/** One word per row; the chip's tooltip has the whole sentence with the time. */
const BRIDGE_WORD: Record<BridgeState, string> = { connected: "connected", connecting: "connecting", waiting: "waiting", off: "off" };

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
export function PortsPage() {
  const stacks = useStore((state) => state.stacks);
  const machines = useStore((state) => state.machines);
  const statuses = useStore((state) => state.statuses);
  const forwards = useStore((state) => state.forwards);
  const setStacks = useStore((state) => state.setStacks);
  // The clock only feeds "(3m)" in a connected bridge's tooltip, which changes once a minute.
  const anyConnected = stacks.some((stack) => stack.forward_ports && forwards[stack.id]?.up);
  const now = useNow(30_000, anyConnected);
  // One action at a time on this page; `target` says whose button spins: a stack's id, or "all".
  const action = useAction("notice");
  const [target, setTarget] = useState<string | null>(null);
  const busyWith = (id: string) => action.busy && target === id;

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
        rows.push({
          local: stack.port_overrides[String(port.published)] ?? port.published,
          remote: port.published,
          stack,
          machine,
          service: service.service,
          bridge,
        });
      }
    }
  }
  rows.sort((a, b) => a.local - b.local);
  const connectedCount = rows.filter((row) => row.bridge.state === "connected").length;
  // The switch acts on the whole stack, so only a stack's first row carries
  // it, wherever its other ports land in the order.
  const rowsWithSwitch = new Set<number>();
  const stacksSeen = new Set<string>();
  rows.forEach((row, index) => {
    if (stacksSeen.has(row.stack.id)) return;
    stacksSeen.add(row.stack.id);
    rowsWithSwitch.add(index);
  });

  const flip = (stack: Stack) => {
    setTarget(stack.id);
    void action.run(async () => setStacks(await api.setForwardPorts(stack.id, !stack.forward_ports)));
  };
  const restartAll = () => {
    setTarget("all");
    void action.run(api.resetForwards);
  };
  const summary =
    rows.length === 0 ? "What localhost points at on this computer" : `${connectedCount} of ${plural(rows.length, "port")} connected to their machines`;

  return (
    <Page
      title="Ports"
      summary={summary}
      actions={
        <Button
          onClick={restartAll}
          busy={busyWith("all")}
          disabled={action.busy || rows.length === 0}
          title="Drop every bridge and let the running stacks bring theirs back"
        >
          <RefreshIcon /> Restart all bridges
        </Button>
      }
    >
      {rows.length === 0 ? (
        <EmptyPanel title="No ports yet">Start or copy a stack, and each port it publishes shows up here as a localhost address on this computer.</EmptyPanel>
      ) : (
        <div className="overflow-hidden rounded-xl border border-line bg-surface">
          <table className="w-full text-[12px]">
            <thead className="whitespace-nowrap bg-surface-2 text-[11px] text-ink-3">
              <tr>
                <th className="px-4 py-2.5 text-left font-medium">On this computer</th>
                <th className="px-2 py-2.5 text-left font-medium">Goes to</th>
                <th className="px-2 py-2.5 text-left font-medium">Stack</th>
                <th className="px-2 py-2.5 text-left font-medium">
                  <Term name="bridge" />
                </th>
                <th className="px-4 py-2.5 text-right font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row, index) => {
                const connected = row.bridge.state === "connected";
                const firstOfStack = rowsWithSwitch.has(index);
                return (
                  <tr key={`${row.stack.id}-${row.local}`} className="border-t border-line">
                    <td className="px-4 py-2.5">
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
                    <td className="mono px-2 py-2.5 text-ink-2">
                      {row.machine?.name ?? "a removed machine"}:{row.remote}
                    </td>
                    <td className="px-2 py-2.5 text-ink-2">
                      {row.stack.name}
                      {row.service ? <span className="text-ink-3"> / {row.service}</span> : null}
                    </td>
                    <td className="px-2 py-2.5">
                      <Chip tone={row.bridge.tone} title={row.bridge.text} className="whitespace-nowrap">
                        {BRIDGE_WORD[row.bridge.state]}
                      </Chip>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-right">
                      {firstOfStack ? (
                        <Button
                          size="sm"
                          tone="ghost"
                          onClick={() => flip(row.stack)}
                          busy={busyWith(row.stack.id)}
                          disabled={action.busy}
                          title={
                            row.stack.forward_ports
                              ? `All of ${row.stack.name}'s ports close here; the stack keeps running`
                              : `Bring ${row.stack.name}'s ports to localhost`
                          }
                        >
                          {row.stack.forward_ports ? "Stop bridge" : "Start bridge"}
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Page>
  );
}
