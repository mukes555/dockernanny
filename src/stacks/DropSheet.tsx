import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { dropTarget, visibleMachines } from "../lib/machines";
import type { Preview, Stack } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog, DialogActions } from "../ui/Dialog";
import { Button, Chip, Field, Select, TextInput, Toggle } from "../ui/primitives";
import { BindMounts, excludeFor } from "./BindMounts";

/** Confirms a dropped compose file: where it runs, what it publishes, what to skip. */
export function DropSheet() {
  const preview = useStore((state) => state.preview);
  const setPreview = useStore((state) => state.setPreview);
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stats = useStore((state) => state.stats);
  // Records that point back at this computer are hidden everywhere else too.
  const machines = visibleMachines(allMachines, computerInfo);
  const setStacks = useStore((state) => state.setStacks);
  const clearOutput = useStore((state) => state.clearOutput);

  const [name, setName] = useState("");
  const [machineId, setMachineId] = useState("");
  const [excludes, setExcludes] = useState("");
  // Mounted folders left to the machine; those it alone has start here.
  const [skipped, setSkipped] = useState<string[]>([]);
  const [forward, setForward] = useState(true);
  const [live, setLive] = useState(false);
  const [overrides, setOverrides] = useState<Record<number, number>>({});
  const [busy, setBusy] = useState<number[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fresh form for every drop, with the ports checked against this computer,
  // on the machine the drop overlay named. Only a new drop resets the form:
  // a machine list refreshed while it is open must not throw away what was typed.
  useEffect(() => {
    if (!preview) return;
    setName(preview.name);
    const state = useStore.getState();
    const target = dropTarget(visibleMachines(state.machines, state.computerInfo), state.stats, state.selectedMachineId);
    setMachineId(target?.id ?? "");
    setOverrides({});
    setSkipped(preview.binds.filter((bind) => !bind.exists_here).map((bind) => bind.path));
    setError(null);
    void api.defaultExcludes().then((list) => setExcludes(list.join(", ")));
    void api.busyPorts(allPorts(preview)).then(setBusy);
  }, [preview]);

  if (!preview) return null;
  const machine = machines.find((m) => m.id === machineId);
  const localFor = (published: number) => overrides[published] ?? published;

  const setLocal = (published: number, value: number) => {
    const next = { ...overrides };
    if (value === published || !value) delete next[published];
    else next[published] = value;
    setOverrides(next);
    void api.busyPorts(allPorts(preview).map((p) => (p === published ? value : localFor(p)))).then(setBusy);
  };

  const run = async () => {
    setSubmitting(true);
    setError(null);
    const stack: Stack = {
      id: Math.random().toString(16).slice(2, 10),
      name,
      machine_id: machineId,
      project_dir: preview.project_dir,
      compose_rel: preview.compose_rel,
      excludes: [
        ...excludes
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
        ...preview.binds.filter((bind) => skipped.includes(bind.path)).map(excludeFor),
      ],
      forward_ports: forward,
      live_sync: live,
      port_overrides: Object.fromEntries(Object.entries(overrides).map(([k, v]) => [String(k), v])),
    };
    try {
      clearOutput(stack.id);
      setStacks(await api.createStack(stack));
      setPreview(null);
      // The new card is where the sync and the build show; go to it, wherever the drop happened.
      useStore.getState().selectMachine(machineId);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open onClose={() => setPreview(null)} eyebrow="New stack" title={preview.compose_rel} width={640} closeOnBackdrop={false}>
      <p className="mono mt-1 truncate text-[11px] text-ink-3" title={preview.project_dir}>
        {preview.project_dir}
      </p>
      {machines.length === 0 ? (
        <div className="mt-4 flex items-center justify-between gap-3 rounded-xl border border-warning/40 bg-warning/10 px-3 py-2.5 text-[12px] text-warning">
          <span>There is no machine to run it on yet. Add one, then drop the file again.</span>
          <Button
            size="sm"
            onClick={() => {
              setPreview(null);
              useStore.getState().setAddMachineOpen(true);
            }}
          >
            Add machine
          </Button>
        </div>
      ) : null}

      <div className="mt-5 grid grid-cols-2 gap-3">
        <Field label="Stack name" hint="Also the compose project name on the machine.">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label="Run on">
          <Select value={machineId} onChange={(e) => setMachineId(e.target.value)}>
            {machines.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} ({m.user}@{m.host}){stats[m.id]?.online ? "" : ", offline"}
              </option>
            ))}
          </Select>
        </Field>
      </div>

      <div className="mt-5 overflow-x-auto rounded-xl border border-line">
        <table className="w-full text-[12px]">
          <thead className="whitespace-nowrap bg-surface-2 text-[10px] uppercase tracking-[0.12em] text-ink-3">
            <tr>
              <th className="px-3 py-2 text-left font-medium">Service</th>
              <th className="px-3 py-2 text-left font-medium">Image</th>
              <th className="px-3 py-2 text-left font-medium">On the machine</th>
              <th className="px-3 py-2 text-left font-medium">On this computer</th>
            </tr>
          </thead>
          <tbody>
            {preview.services.map((service) => (
              <tr key={service.name} className="border-t border-line align-top">
                <td className="px-3 py-2 font-medium text-ink">{service.name}</td>
                <td className="mono px-3 py-2 text-ink-2">{service.builds ? "build" : (service.image ?? "")}</td>
                <td className="px-3 py-2 text-ink-2">
                  {service.ports.length === 0 ? <span className="text-ink-3">no ports</span> : null}
                  {service.ports.map((port) => (
                    <div key={port.published} className="tabular leading-7">
                      :{port.published} <span className="text-ink-3">to {port.target}</span>
                    </div>
                  ))}
                </td>
                <td className="px-3 py-2">
                  {service.ports.map((port) => {
                    const local = localFor(port.published);
                    const taken = busy.includes(local);
                    return (
                      <div key={port.published} className="flex items-center gap-2 leading-7">
                        <span className="text-ink-3">localhost:</span>
                        <input
                          aria-label={`Port on this computer for ${service.name} ${port.published}`}
                          className="tabular w-16 rounded-md border border-line bg-surface-2 px-1.5 py-0.5 text-[12px] text-ink outline-none focus:border-accent"
                          value={local}
                          onChange={(e) => setLocal(port.published, Number(e.target.value.replace(/\D/g, "")))}
                        />
                        {taken ? (
                          <button type="button" className="whitespace-nowrap text-[11px] text-warning underline-offset-2 hover:underline" onClick={() => setLocal(port.published, port.published + 1000)}>
                            in use here, try {port.published + 1000}
                          </button>
                        ) : null}
                      </div>
                    );
                  })}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {preview.warnings.length > 0 || preview.has_env_file ? (
        <ul className="mt-3 space-y-1 text-[12px] text-ink-2">
          {preview.warnings.map((warning) => (
            <li key={warning} className="flex gap-2">
              <Chip tone="warning">note</Chip>
              <span>{warning}</span>
            </li>
          ))}
          {preview.has_env_file ? (
            <li className="flex gap-2">
              <Chip tone="neutral">.env</Chip>
              <span>The .env file is copied to the machine with the project.</span>
            </li>
          ) : null}
        </ul>
      ) : null}

      <BindMounts binds={preview.binds} skipped={skipped} onSkip={(path, skip) => setSkipped((list) => (skip ? [...list, path] : list.filter((p) => p !== path)))} />

      <div className="mt-4">
        <Field label="Do not copy" hint="Comma separated. Excluded folders the containers create on the machine are kept.">
          <TextInput value={excludes} onChange={(e) => setExcludes(e.target.value)} className="mono" />
        </Field>
      </div>
      <div className="mt-4 flex flex-wrap gap-5">
        <Toggle checked={forward} onChange={setForward} label="Bridge its ports to localhost" />
        <Toggle checked={live} onChange={setLive} label="Re-sync when files change" />
      </div>

      <DialogActions error={error}>
        <Button tone="ghost" onClick={() => setPreview(null)}>
          Cancel
        </Button>
        <Button tone="primary" onClick={() => void run()} busy={submitting} disabled={!machine || !name.trim()}>
          Run on {machine?.name ?? "…"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function allPorts(preview: Preview): number[] {
  return preview.services.flatMap((service) => service.ports.map((port) => port.published));
}
