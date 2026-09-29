import { useEffect, useState } from "react";

import { api } from "../lib/ipc";
import { dropTarget, visibleMachines } from "../lib/machines";
import type { Preview, Stack } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog, DialogActions } from "../ui/Dialog";
import { Button, Chip, Field, LIST_HEAD, Select, TextInput, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { BindMounts, excludeFor } from "./BindMounts";
import { LocalPortField, usePortOverrides } from "./PortOverrides";

/** Confirms a dropped compose file: where it runs, what it publishes, what
 * to skip. The form mounts with each drop, so it starts fresh every time, on
 * the machine the drop overlay named; a machine list refreshed while it is
 * open does not throw away what was typed. */
export function DropSheet() {
  const preview = useStore((state) => state.preview);
  const setPreview = useStore((state) => state.setPreview);
  return (
    <Dialog open={preview !== null} onClose={() => setPreview(null)} eyebrow="New stack" title={preview?.compose_rel ?? ""} width={640} closeOnBackdrop={false}>
      {preview ? <DropForm key={preview.project_dir + preview.compose_rel} preview={preview} onClose={() => setPreview(null)} /> : null}
    </Dialog>
  );
}

function DropForm({ preview, onClose }: { preview: Preview; onClose: () => void }) {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stats = useStore((state) => state.stats);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const setStacks = useStore((state) => state.setStacks);
  const selectMachine = useStore((state) => state.selectMachine);
  const setAddMachineOpen = useStore((state) => state.setAddMachineOpen);
  // Records that point back at this computer are hidden everywhere else too.
  const machines = visibleMachines(allMachines, computerInfo);

  const [name, setName] = useState(preview.name);
  const [machineId, setMachineId] = useState(() => dropTarget(machines, stats, selectedMachineId)?.id ?? "");
  const [excludes, setExcludes] = useState("");
  // Mounted folders left to the machine; those it alone has start here.
  const [skipped, setSkipped] = useState<string[]>(() => preview.binds.filter((bind) => !bind.exists_here).map((bind) => bind.path));
  const [forward, setForward] = useState(true);
  const [live, setLive] = useState(false);
  const ports = usePortOverrides(allPorts(preview));
  const creating = useAction("inline");

  useEffect(() => {
    api
      .defaultExcludes()
      .then((list) => setExcludes(list.join(", ")))
      .catch(console.warn);
  }, []);

  const machine = machines.find((m) => m.id === machineId);

  const run = async () => {
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
      port_overrides: ports.forRequest,
    };
    const created = await creating.run(async () => setStacks(await api.createStack(stack)));
    if (!created) return;
    onClose();
    // The new card is where the sync and the build show; go to it, wherever the drop happened.
    selectMachine(machineId);
  };

  return (
    <>
      <p className="mono mt-1 truncate text-[11px] text-ink-3" title={preview.project_dir}>
        {preview.project_dir}
      </p>
      {machines.length === 0 ? (
        <div className="mt-4 flex items-center justify-between gap-3 rounded-xl border border-warning/40 bg-warning/10 px-3 py-2.5 text-[12px] text-warning">
          <span>There is no machine to run it on yet. Add one, then drop the file again.</span>
          <Button
            size="sm"
            onClick={() => {
              onClose();
              setAddMachineOpen(true);
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
          <thead className="whitespace-nowrap">
            <tr>
              <th className={LIST_HEAD}>Service</th>
              <th className={LIST_HEAD}>Image</th>
              <th className={LIST_HEAD}>On the machine</th>
              <th className={LIST_HEAD}>On this computer</th>
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
                    <div key={`${port.published}-${port.protocol}`} className="tabular leading-7">
                      :{port.published} <span className="text-ink-3">to {port.target}</span>
                      {port.protocol === "udp" ? <span className="text-ink-3"> udp</span> : null}
                    </div>
                  ))}
                </td>
                <td className="px-3 py-2">
                  {service.ports.map((port) => (
                    <div key={`${port.published}-${port.protocol}`} className="flex items-center gap-2 leading-7">
                      {port.protocol === "udp" ? (
                        <span className="text-ink-3">not bridged (UDP)</span>
                      ) : (
                        <>
                          <span className="text-ink-3">localhost:</span>
                          <LocalPortField
                            port={port.published}
                            local={ports.localFor(port.published)}
                            taken={ports.isTaken(port.published)}
                            label={`Port on this computer for ${service.name} ${port.published}`}
                            onChange={(value) => ports.setLocal(port.published, value)}
                          />
                        </>
                      )}
                    </div>
                  ))}
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

      <BindMounts
        binds={preview.binds}
        skipped={skipped}
        onSkip={(path, skip) => setSkipped((list) => (skip ? [...list, path] : list.filter((p) => p !== path)))}
      />

      <div className="mt-4">
        <Field label="Do not copy" hint="Comma separated. Excluded folders the containers create on the machine are kept.">
          <TextInput value={excludes} onChange={(e) => setExcludes(e.target.value)} className="mono" />
        </Field>
      </div>
      <div className="mt-4 flex flex-wrap gap-5">
        <Toggle checked={forward} onChange={setForward} label="Bridge its ports to localhost" />
        <Toggle checked={live} onChange={setLive} label="Re-sync when files change" />
      </div>

      <DialogActions error={creating.error}>
        <Button tone="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button tone="primary" onClick={() => void run()} busy={creating.busy} disabled={!machine || !name.trim()}>
          Run on {machine?.name ?? "…"}
        </Button>
      </DialogActions>
    </>
  );
}

/** The TCP ports the stack publishes, each once: what the bridge carries to localhost. */
function allPorts(preview: Preview): number[] {
  const tcp = preview.services.flatMap((service) => service.ports.filter((port) => port.protocol !== "udp").map((port) => port.published));
  return [...new Set(tcp)];
}
