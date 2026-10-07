import { describe, expect, it } from "vitest";

import { dropTarget, hiddenMachines, visibleMachines } from "./machines";
import type { ComputerInfo, Machine, MachineStats, Probe } from "./types";

function machine(id: string, host: string): Machine {
  return { id, name: id, user: "alex", host, port: 22, key_path: "", docker_context: false, pinned: false };
}

const probe: Probe = {
  hostname: "Studio",
  os: null,
  cpu_model: null,
  cpus: 8,
  load1: 0,
  uptime_s: 0,
  mem_used_mb: 0,
  mem_total_mb: 0,
  disk_free_bytes: 0,
  disk_total_bytes: 0,
  battery: null,
  docker_version: null,
  flavor: null,
  containers_running: 0,
};

const thisComputer: ComputerInfo = { name: "Studio", user: "alex", probe };

function stats(online: boolean): MachineStats {
  return { ...probe, online, error: null };
}

describe("machines", () => {
  const box = machine("box", "192.0.2.10");
  const loopback = machine("loop", "localhost");
  const ownName = machine("own", "studio.local");

  it("keeps records that point back at this computer out of the list", () => {
    const all = [box, loopback, ownName];
    expect(visibleMachines(all, thisComputer)).toEqual([box]);
    expect(hiddenMachines(all, thisComputer)).toEqual([loopback, ownName]);
  });

  it("hides only loopback addresses before this computer's name is known", () => {
    expect(visibleMachines([box, loopback, ownName], null)).toEqual([box, ownName]);
  });

  it("drops onto the open machine, else the first that answers, else the first", () => {
    const other = machine("other", "192.0.2.11");
    const machines = [box, other];
    expect(dropTarget(machines, { other: stats(true) }, "box")).toBe(box);
    expect(dropTarget(machines, { box: stats(false), other: stats(true) }, null)).toBe(other);
    expect(dropTarget(machines, {}, null)).toBe(box);
    expect(dropTarget([], {}, null)).toBeUndefined();
  });
});
