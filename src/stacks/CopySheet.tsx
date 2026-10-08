import { useState } from "react";

import { api } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import type { CopyRequest, EndpointRef, LocalProject, Machine, Stack } from "../lib/types";
import type { CopyIntent } from "../state/store";
import { useStore } from "../state/store";
import { Dialog, DialogActions } from "../ui/Dialog";
import { ArrowLeftIcon } from "../ui/icons";
import { Button, Chip, cx, Eyebrow, Field, Inset, LIST_HEAD, TextInput, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { useLoaded } from "../ui/useLoaded";
import { CopyData, splitSelectionKey } from "./CopyData";
import { EndpointPills, Section, SourceModeRadio, SourcePicker } from "./CopyPickers";
import type { SourceMode } from "./CopyPickers";
import { LocalPortField, usePortOverrides } from "./PortOverrides";
import { useCopyPlan } from "./useCopyPlan";

const THIS_COMPUTER: EndpointRef = { kind: "this_computer" };
const onMachine = (machine_id: string): EndpointRef => ({ kind: "machine", machine_id });

/** One flow for moving a stack's config and data between this computer and
 * the machines, in either direction. The backend plans first (what travels,
 * what gets replaced) and the sheet shows that before anything happens.
 * The form mounts with the sheet, so every opening starts from what it was
 * opened for. */
export function CopySheet() {
  const intent = useStore((state) => state.copy);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const close = () => setCopyOpen({ open: false });
  return (
    <Dialog open={intent.open} onClose={close} eyebrow="Copy" title="Copy a stack" width={720} closeOnBackdrop={false}>
      <CopyForm intent={intent} onClose={close} />
    </Dialog>
  );
}

interface Start {
  source: EndpointRef;
  stackId: string | null;
  name: string;
  folder: string;
  destination: EndpointRef | null;
}

/** Where the sheet starts: a stack card preselects its stack, a machine page
 * its machine as the destination, this computer's page a way back here. */
function startFrom(intent: CopyIntent, stacks: Stack[], machines: Machine[], online: (id: string) => boolean): Start {
  const sourceStack = stacks.find((s) => s.id === intent.sourceStackId);
  const askedDestination = intent.destinationMachineId ? onMachine(intent.destinationMachineId) : null;
  if (sourceStack) {
    return {
      source: onMachine(sourceStack.machine_id),
      stackId: sourceStack.id,
      name: sourceStack.name,
      folder: sourceStack.project_dir,
      destination: askedDestination ?? THIS_COMPUTER,
    };
  }
  if (intent.toThisComputer) {
    const firstOnline = machines.find((m) => online(m.id)) ?? machines[0];
    return { source: firstOnline ? onMachine(firstOnline.id) : THIS_COMPUTER, stackId: null, name: "", folder: "", destination: THIS_COMPUTER };
  }
  const firstMachine = machines[0] ? onMachine(machines[0].id) : null;
  return { source: THIS_COMPUTER, stackId: null, name: "", folder: "", destination: askedDestination ?? firstMachine };
}

function CopyForm({ intent, onClose }: { intent: CopyIntent; onClose: () => void }) {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stats = useStore((state) => state.stats);
  const stacks = useStore((state) => state.stacks);
  const setStacks = useStore((state) => state.setStacks);
  const clearOutput = useStore((state) => state.clearOutput);
  const openProgress = useStore((state) => state.openProgress);
  const machines = visibleMachines(allMachines, computerInfo);
  const isOnline = (id: string) => stats[id]?.online ?? false;

  const [start] = useState(() => startFrom(intent, stacks, machines, isOnline));
  const [source, setSource] = useState(start.source);
  const [stackId, setStackId] = useState(start.stackId);
  const [project, setProject] = useState<LocalProject | null>(null);
  const [destination, setDestination] = useState(start.destination);
  const [name, setName] = useState(start.name);
  const [folder, setFolder] = useState(start.folder);
  const [config, setConfig] = useState(true);
  const [data, setData] = useState(true);
  const [mode, setMode] = useState<SourceMode>("stop");
  const local = useLoaded(api.localProjects);
  const loose = useLoaded(api.looseContainers);
  const copying = useAction("inline");

  // Opened from one of this computer's projects: pick it once the list is read.
  const [presetDone, setPresetDone] = useState(!intent.sourceProject);
  if (!presetDone && local.data) {
    setPresetDone(true);
    const preset = local.data.find((p) => p.name === intent.sourceProject);
    if (preset) {
      setProject(preset);
      setName(preset.name);
    }
  }

  const sourceStack = stacks.find((s) => s.id === stackId) ?? null;
  const sourceChosen = source.kind === "this_computer" ? project !== null : sourceStack !== null;
  const leaving = mode === "leave";
  // What the plan is asked for; the ticks and the ports are added once it answers.
  const planned: CopyRequest | null =
    destination && sourceChosen && name.trim()
      ? {
          source,
          project: source.kind === "this_computer" ? (project ?? undefined) : undefined,
          stack_id: source.kind === "machine" ? (stackId ?? undefined) : undefined,
          destination,
          folder: destination.kind === "this_computer" ? folder : "",
          name: name.trim(),
          config,
          data,
          data_selection: [],
          stop_source: mode !== "keep",
          keep_source_stopped: leaving,
          port_overrides: {},
          excludes: [],
          forward_ports: true,
        }
      : null;
  const { plan, planning, error: planError, ticked, setTicked } = useCopyPlan(planned);
  const toMachine = destination?.kind === "machine";
  const ports = usePortOverrides(toMachine && plan ? plan.ports : []);
  // A move frees the source's ports, so nothing here can be in their way.
  const blocked = !leaving && ports.conflicts.length > 0;
  const request: CopyRequest | null = planned
    ? { ...planned, data_selection: [...ticked].map(splitSelectionKey), port_overrides: leaving ? {} : ports.forRequest }
    : null;

  const machineName = (ref: EndpointRef) => (ref.kind === "machine" ? (machines.find((m) => m.id === ref.machine_id)?.name ?? "machine") : "this computer");
  const sourceName = source.kind === "this_computer" ? project?.name : sourceStack?.name;
  const canCopy = request !== null && plan !== null && !planning && !blocked && (config || data);

  const copy = async () => {
    if (!request) return;
    await copying.run(async () => {
      const started = await api.copyStack(request);
      // The card the copy lands on starts with an empty output; the other cards keep theirs.
      clearOutput(started.card_id);
      setStacks(started.stacks);
      onClose();
      openProgress(started.card_id);
    });
  };

  const pickSource = (ref: EndpointRef) => {
    setSource(ref);
    setProject(null);
    setStackId(null);
  };
  const pickProject = (picked: LocalProject) => {
    setProject(picked);
    // A name the user typed stays; one that only followed the last pick follows this one.
    if (!name || name === project?.name) setName(picked.name);
  };
  const pickStack = (picked: Stack) => {
    setStackId(picked.id);
    if (!name || name === sourceStack?.name) setName(picked.name);
    setFolder(picked.project_dir);
  };

  return (
    <>
      <p className="mt-1 text-[13px] text-ink-2">
        Config is the project folder; data is the volumes and what the containers keep inside. Nothing at the source is deleted.
      </p>

      {sourceName && destination ? (
        <Inset className="mt-3 flex flex-wrap items-center gap-2 text-[12px]">
          <span className="text-ink-3">{machineName(source)}</span>
          <span className="font-medium text-ink">{sourceName}</span>
          <ArrowLeftIcon size={13} className="rotate-180 text-ink-3" />
          <span className="text-ink-3">{machineName(destination)}</span>
          <span className="font-medium text-ink">{name.trim() || sourceName}</span>
          <span className="ml-auto flex gap-1.5">
            {config ? <Chip tone="accent">config</Chip> : null}
            {data ? <Chip tone="accent">data</Chip> : null}
            {leaving ? <Chip tone="warning">move</Chip> : null}
          </span>
        </Inset>
      ) : null}

      <Section step="1" title="From">
        <EndpointPills chosen={source} machines={machines} online={isOnline} exclude={null} onPick={pickSource} />
        <SourcePicker
          source={source}
          projects={local.data}
          looseNames={loose.data ?? []}
          stacks={stacks}
          project={project}
          stackId={stackId}
          onProject={pickProject}
          onStack={pickStack}
        />
      </Section>

      <Section step="2" title="To">
        <EndpointPills chosen={destination} machines={machines} online={isOnline} exclude={source} onPick={setDestination} />
        <div className="mt-3 grid grid-cols-2 gap-3">
          <Field label="Name at the destination">
            <TextInput value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          {destination?.kind === "this_computer" ? (
            <Field label="Folder on this computer" hint="Where the project folder lands.">
              <TextInput value={folder} onChange={(e) => setFolder(e.target.value)} className="mono" placeholder="/home/alex/projects/shop" />
            </Field>
          ) : null}
        </div>
        {plan?.destination_exists && destination ? (
          <div className="mt-3 rounded-lg bg-warning/10 px-3 py-2 text-[12px] leading-relaxed text-warning">
            {destination.kind === "this_computer" ? (
              <>
                <span className="mono">{folder}</span> already has this project. Its files are replaced by the ones from {machineName(source)}, files that exist
                only here are deleted (excluded folders like .git and node_modules stay), and its data is replaced. Choose another folder to keep both.
              </>
            ) : (
              <>
                {machineName(destination)} already has {name}: its folder becomes an exact copy (files only there are deleted) and its data is replaced.
              </>
            )}
          </div>
        ) : null}
      </Section>

      <Section step="3" title="What travels">
        <div className="flex flex-wrap gap-4">
          <Toggle checked={config} onChange={setConfig} label="Config (the project folder)" />
          <Toggle checked={data} onChange={setData} label="Data (volumes and what the containers keep inside)" />
        </div>
        {data ? (
          <div className="mt-3">
            <CopyData
              volumes={plan?.volumes ?? []}
              containers={plan?.containers ?? []}
              loading={planning}
              selected={ticked}
              onToggle={(key, on) =>
                setTicked((current) => {
                  const next = new Set(current);
                  if (on) next.add(key);
                  else next.delete(key);
                  return next;
                })
              }
            />
          </div>
        ) : null}
        {plan?.images.length ? (
          <div className="mt-2 text-[12px] text-ink-2">Images that only exist at the source are sent along: {plan.images.join(", ")}.</div>
        ) : null}

        <div className="mt-3 space-y-1 text-[12px]">
          <Eyebrow>The source during the copy</Eyebrow>
          <SourceModeRadio
            value="stop"
            mode={mode}
            onChange={setMode}
            label={`Stop it for the data copy, then start it again${plan && !plan.source_running ? " (it is not running now)" : ""}`}
          />
          <SourceModeRadio value="leave" mode={mode} onChange={setMode} label="Stop it and leave it stopped: a move, which also frees its ports" />
          <SourceModeRadio
            value="keep"
            mode={mode}
            onChange={setMode}
            label="Keep it running: fast, but a database copied while it writes may be inconsistent"
          />
        </div>

        {toMachine && plan && plan.ports.length > 0 ? (
          <div className="mt-3 rounded-xl border border-line">
            <div className={cx("border-b border-line", LIST_HEAD)}>Ports on this computer afterwards</div>
            {plan.ports.map((port) => (
              <div key={port} className="flex items-center gap-2 px-3 py-1.5 text-[12px]">
                <span className="tabular w-16 text-ink-2">:{port}</span>
                <span className="text-ink-3">becomes localhost:</span>
                <LocalPortField
                  port={port}
                  local={leaving ? port : ports.localFor(port)}
                  taken={!leaving && ports.isTaken(port)}
                  disabled={leaving}
                  label={`Port on this computer for ${port}`}
                  onChange={(value) => ports.setLocal(port, value)}
                />
              </div>
            ))}
          </div>
        ) : null}

        {plan?.warnings.map((warning) => (
          <div key={warning} className="mt-2 flex gap-2 text-[12px] text-ink-2">
            <Chip tone="warning">note</Chip>
            <span>{warning}</span>
          </div>
        ))}
        {plan?.notes.length ? <div className="mt-2 text-[11px] text-ink-3">{plan.notes.join(" · ")}</div> : null}
      </Section>

      <DialogActions error={copying.error ?? planError ?? local.error}>
        <Button tone="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button
          tone="primary"
          onClick={() => void copy()}
          busy={copying.busy}
          disabled={!canCopy}
          title={blocked ? "Pick other local ports or leave the source stopped" : undefined}
        >
          Copy to {destination ? machineName(destination) : "…"}
        </Button>
      </DialogActions>
    </>
  );
}
