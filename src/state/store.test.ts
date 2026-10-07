import { beforeEach, describe, expect, it, vi } from "vitest";

import type { OutputLine, StackStatus } from "../lib/types";
import { isBusy, isUp, useStore } from "./store";

// What is tested here only changes state; nothing may reach a backend, the pretend one included.
vi.mock("../lib/ipc", () => ({ api: {}, errorMessage: String }));

const initial = useStore.getState();

function lines(count: number, prefix = ""): OutputLine[] {
  return Array.from({ length: count }, (_, i) => ({ stream: "stdout", text: `${prefix}line ${i}` }));
}

function status(phase: StackStatus["phase"]): StackStatus {
  return { phase, services: [], error: null, sync_warning: null, synced_at_ms: null, synced_files: 0, known: true };
}

describe("store", () => {
  beforeEach(() => useStore.setState(initial, true));

  it("keeps the last 400 lines of a card's output", () => {
    const { appendOutput } = useStore.getState();
    appendOutput("s", lines(300));
    appendOutput("s", lines(300, "more "));
    const kept = useStore.getState().output.s;
    expect(kept).toHaveLength(400);
    expect(kept[kept.length - 1].text).toBe("more line 299");
  });

  it("parses a stack's log lines once, numbers them, and drops lines for a closed drawer", () => {
    const store = useStore.getState();
    store.openLogs("s");
    store.appendLog("s", [{ stream: "stdout", text: "shop-db-1  | ready" }]);
    store.appendLog("other", [{ stream: "stdout", text: "late | line" }]);
    const [entry, ...rest] = useStore.getState().logs;
    expect(rest).toHaveLength(0);
    expect(entry).toMatchObject({ container: "shop-db-1", text: "ready", stream: "stdout" });

    store.appendLog("s", [{ stream: "stderr", text: "api | boom" }]);
    const [first, second] = useStore.getState().logs;
    expect(second.seq).toBeGreaterThan(first.seq);
  });

  it("keeps a container's log text whole, since docker logs has no prefix", () => {
    const store = useStore.getState();
    store.openContainerLogs({ machineId: "m", id: "c1", name: "db" });
    store.appendContainerLog("c1", [{ stream: "stdout", text: "a | b" }]);
    expect(useStore.getState().containerLog[0]).toMatchObject({ container: "", text: "a | b" });
  });

  it("opens one drawer at a time", () => {
    const store = useStore.getState();
    store.openLogs("s");
    store.openProgress("s");
    expect(useStore.getState()).toMatchObject({ progressFor: "s", logsFor: null });
    store.openContainerLogs({ machineId: "m", id: "c1", name: "db" });
    expect(useStore.getState()).toMatchObject({ progressFor: null, containerLogsFor: { id: "c1" } });
  });

  it("moves a repeated notice to the end instead of showing it twice, and keeps the last four", () => {
    const { pushNotice } = useStore.getState();
    for (const text of ["a", "b", "a", "c", "d", "e"]) pushNotice(text);
    expect(useStore.getState().notices.map((n) => n.text)).toEqual(["a", "c", "d", "e"]);
  });

  it("keeps the machine page's tab when its own dialog opens, and starts on stacks from elsewhere", () => {
    const store = useStore.getState();
    store.selectMachine("m");
    store.setMachineTab("containers");
    store.openMachineDialog("m", "terminal");
    expect(useStore.getState()).toMatchObject({ machineTab: "containers", machineDialog: "terminal" });
    store.openMachineDialog("other", "remove");
    expect(useStore.getState()).toMatchObject({ selectedMachineId: "other", machineTab: "stacks", machineDialog: "remove" });
  });

  it("tells busy from up", () => {
    expect(isBusy(status("migrating"))).toBe(true);
    expect(isBusy(status("running"))).toBe(false);
    expect(isBusy(undefined)).toBe(false);
    expect(isUp(status("partial"))).toBe(true);
    expect(isUp(status("stopping"))).toBe(false);
  });
});
