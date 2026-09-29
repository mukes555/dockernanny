import type { ComputerInfo, Machine, MachineStats } from "./types";

/** A record that points back at this computer: a test leftover, or a stack
 * someone runs "remotely" on their own Docker. Not a machine to list. */
function pointsAtThisComputer(machine: Machine, info: ComputerInfo | null): boolean {
  const host = machine.host.trim().toLowerCase();
  if (["localhost", "127.0.0.1", "::1", "0.0.0.0"].includes(host)) return true;
  const own = info?.probe.hostname?.toLowerCase() ?? "";
  return own !== "" && (host === own || host === `${own}.local`);
}

export function visibleMachines(machines: Machine[], info: ComputerInfo | null): Machine[] {
  return machines.filter((m) => !pointsAtThisComputer(m, info));
}

export function hiddenMachines(machines: Machine[], info: ComputerInfo | null): Machine[] {
  return machines.filter((m) => pointsAtThisComputer(m, info));
}

/** Where a dropped compose file runs unless the user picks another machine:
 * the machine whose page is open, else the first one that answers, else the
 * first one. The overlay, the empty state and the sheet all name the same one. */
export function dropTarget(machines: Machine[], stats: Record<string, MachineStats>, selectedId: string | null): Machine | undefined {
  const selected = machines.find((m) => m.id === selectedId);
  const firstOnline = machines.find((m) => stats[m.id]?.online);
  return selected ?? firstOnline ?? machines[0];
}
