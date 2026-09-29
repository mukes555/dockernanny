import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import type { CopyPlan, CopyRequest, EndpointRef, LocalProject } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog, DialogActions } from "../ui/Dialog";
import { ArrowLeftIcon, SpinnerIcon } from "../ui/icons";
import { Button, Chip, cx, Field, TextInput, Toggle } from "../ui/primitives";
import { CopyData, defaultSelection, keepOffered, splitSelectionKey } from "./CopyData";

type SourceMode = "keep" | "stop" | "leave";

const sameEndpoint = (a: EndpointRef, b: EndpointRef) => a.kind === b.kind && (a.kind !== "machine" || b.kind !== "machine" || a.machine_id === b.machine_id);

/** One flow for moving a stack's config and data between this computer and
 * the machines, in either direction. The backend plans first (what travels,
 * what gets replaced) and the sheet shows that before anything happens. */
export function CopySheet() {
  const intent = useStore((state) => state.copy);
  const close = () => useStore.getState().setCopyOpen({ open: false });
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const machines = visibleMachines(allMachines, computerInfo);
  const stats = useStore((state) => state.stats);
  const isOnline = (id: string) => stats[id]?.online ?? false;
  const stacks = useStore((state) => state.stacks);
  const setStacks = useStore((state) => state.setStacks);
  const clearOutput = useStore((state) => state.clearOutput);
  const openProgress = useStore((state) => state.openProgress);

  const [projects, setProjects] = useState<LocalProject[] | null>(null);
  const [source, setSource] = useState<EndpointRef>({ kind: "this_computer" });
  const [project, setProject] = useState<LocalProject | null>(null);
  const [stackId, setStackId] = useState<string | null>(null);
  const [destination, setDestination] = useState<EndpointRef | null>(null);
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [config, setConfig] = useState(true);
  const [data, setData] = useState(true);
  const [mode, setMode] = useState<SourceMode>("stop");
  const [plan, setPlan] = useState<CopyPlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  // Which source and destination the ticks were made for.
  const ticksFor = useRef<string | null>(null);
  const [overrides, setOverrides] = useState<Record<number, number>>({});
  const [busy, setBusy] = useState<number[]>([]);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A fresh sheet every time, preselecting what it was opened for.
  useEffect(() => {
    if (!intent.open) return;
    setError(null);
    setPlan(null);
    setOverrides({});
    setConfig(true);
    setData(true);
    setMode("stop");
    const sourceStack = stacks.find((s) => s.id === intent.sourceStackId);
    if (sourceStack) {
      setSource({ kind: "machine", machine_id: sourceStack.machine_id });
      setStackId(sourceStack.id);
      setProject(null);
      setName(sourceStack.name);
      setFolder(sourceStack.project_dir);
      setDestination(intent.destinationMachineId ? { kind: "machine", machine_id: intent.destinationMachineId } : { kind: "this_computer" });
    } else if (intent.toThisComputer) {
      const firstOnline = machines.find((m) => stats[m.id]?.online) ?? machines[0];
      setSource(firstOnline ? { kind: "machine", machine_id: firstOnline.id } : { kind: "this_computer" });
      setStackId(null);
      setProject(null);
      setName("");
      setFolder("");
      setDestination({ kind: "this_computer" });
    } else {
      setSource({ kind: "this_computer" });
      setStackId(null);
      setProject(null);
      setName("");
      setFolder("");
      setDestination(
        intent.destinationMachineId
          ? { kind: "machine", machine_id: intent.destinationMachineId }
          : machines[0]
            ? { kind: "machine", machine_id: machines[0].id }
            : null,
      );
    }
    setProjects(null);
    api
      .localProjects()
      .then((found) => {
        setProjects(found);
        const preset = found.find((p) => p.name === intent.sourceProject);
        if (preset) {
          setProject(preset);
          setName(preset.name);
        }
      })
      .catch((err) => setError(errorMessage(err)));
    // The machine list is derived per render; the intent is what changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [intent, stacks]);

  const sourceStack = stacks.find((s) => s.id === stackId) ?? null;
  const sourceChosen = source.kind === "this_computer" ? project !== null : sourceStack !== null;
  const request = (): CopyRequest | null => {
    if (!destination || !sourceChosen || !name.trim()) return null;
    return {
      source,
      project: source.kind === "this_computer" ? (project ?? undefined) : undefined,
      stack_id: source.kind === "machine" ? (stackId ?? undefined) : undefined,
      destination,
      folder: destination.kind === "this_computer" ? folder : "",
      name: name.trim(),
      config,
      data,
      data_selection: [...ticked].map(splitSelectionKey),
      stop_source: mode !== "keep",
      keep_source_stopped: mode === "leave",
      port_overrides: mode === "leave" ? {} : Object.fromEntries(Object.entries(overrides).map(([k, v]) => [String(k), v])),
      excludes: [],
      forward_ports: true,
    };
  };

  // The plan follows the choices, a moment after they settle.
  const planKey = JSON.stringify({ source, project: project?.name, stackId, destination, name, folder, data });
  // What is copied from where to where; the name and the folder are not part of it.
  const whatKey = JSON.stringify({ source, project: project?.name, stackId, destination });
  useEffect(() => {
    if (!intent.open) return;
    const wanted = request();
    if (!wanted) {
      setPlan(null);
      setPlanning(false);
      return;
    }
    let current = true;
    setPlanning(true);
    const timer = window.setTimeout(() => {
      api
        .copyPlan(wanted)
        .then((found) => {
          if (!current) return;
          setPlan(found);
          // Only another source or destination brings the defaults back: a
          // renamed destination must not re-tick data the user left out.
          const sameWhat = ticksFor.current === whatKey;
          ticksFor.current = whatKey;
          setTicked((previous) => (sameWhat ? keepOffered(previous, found.containers) : defaultSelection(found.containers)));
          setError(null);
          void api.busyPorts(found.ports).then((ports) => current && setBusy(ports));
        })
        .catch((err) => {
          if (!current) return;
          // No plan, no Copy: an old plan must not stand in for one that failed.
          setPlan(null);
          setError(errorMessage(err));
        })
        .finally(() => current && setPlanning(false));
    }, 400);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [planKey, intent.open]);

  if (!intent.open) return null;

  const localFor = (port: number) => overrides[port] ?? port;
  const toMachine = destination?.kind === "machine";
  const conflicts = toMachine && plan ? plan.ports.filter((port) => busy.includes(localFor(port))) : [];
  const blocked = mode !== "leave" && conflicts.length > 0;
  const machineName = (ref: EndpointRef) => (ref.kind === "machine" ? (machines.find((m) => m.id === ref.machine_id)?.name ?? "machine") : "this computer");

  const copy = async () => {
    const wanted = request();
    if (!wanted) return;
    setWorking(true);
    setError(null);
    try {
      const started = await api.copyStack(wanted);
      for (const stack of started.stacks) clearOutput(stack.id);
      setStacks(started.stacks);
      close();
      openProgress(started.card_id);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  const sourceName = source.kind === "this_computer" ? project?.name : sourceStack?.name;

  return (
    <Dialog open onClose={close} eyebrow="Copy" title="Copy a stack" width={720} closeOnBackdrop={false}>
      <p className="mt-1 text-[13px] text-ink-2">
        Config is the project folder; data is the volumes and what the containers keep inside. Nothing at the source is deleted.
      </p>

      {sourceName && destination ? (
        <div className="mt-3 flex flex-wrap items-center gap-2 rounded-xl border border-line bg-surface-2/50 px-3 py-2 text-[12px]">
          <span className="text-ink-3">{machineName(source)}</span>
          <span className="font-medium text-ink">{sourceName}</span>
          <ArrowLeftIcon size={13} className="rotate-180 text-ink-3" />
          <span className="text-ink-3">{machineName(destination)}</span>
          <span className="font-medium text-ink">{name.trim() || sourceName}</span>
          <span className="ml-auto flex gap-1.5">
            {config ? <Chip tone="accent">config</Chip> : null}
            {data ? <Chip tone="accent">data</Chip> : null}
            {mode === "leave" ? <Chip tone="warning">move</Chip> : null}
          </span>
        </div>
      ) : null}

      <Section step="1" title="From">
        <EndpointPills
          chosen={source}
          machines={machines}
          online={isOnline}
          exclude={null}
          onPick={(ref) => {
            setSource(ref);
            setProject(null);
            setStackId(null);
            setPlan(null);
          }}
        />
        {source.kind === "this_computer" ? (
          projects === null ? (
            <div className="mt-2 flex items-center gap-2 text-[12px] text-ink-3">
              <SpinnerIcon size={12} /> reading this computer's Docker
            </div>
          ) : projects.length === 0 ? (
            <div className="mt-2 text-[12px] text-ink-3">Docker on this computer has no compose projects right now.</div>
          ) : (
            <div className="mt-2 space-y-1.5">
              {projects.map((p) => (
                <PickRow
                  key={p.name}
                  selected={project?.name === p.name}
                  onClick={() => {
                    setProject(p);
                    if (!name || name === project?.name) setName(p.name);
                  }}
                  title={p.name}
                  subtitle={p.project_dir}
                  chip={p.status}
                  good={p.status.startsWith("running")}
                  extra={`${p.volumes.length === 1 ? "1 volume" : `${p.volumes.length} volumes`} · ${p.ports.length === 1 ? "1 port" : `${p.ports.length} ports`}`}
                />
              ))}
            </div>
          )
        ) : (
          <div className="mt-2 space-y-1.5">
            {stacks
              .filter((s) => source.kind === "machine" && s.machine_id === source.machine_id)
              .map((s) => (
                <PickRow
                  key={s.id}
                  selected={stackId === s.id}
                  onClick={() => {
                    setStackId(s.id);
                    if (!name || name === sourceStack?.name) setName(s.name);
                    setFolder(s.project_dir);
                  }}
                  title={s.name}
                  subtitle={s.project_dir}
                />
              ))}
            {stacks.every((s) => source.kind !== "machine" || s.machine_id !== source.machine_id) ? (
              <div className="text-[12px] text-ink-3">No stacks on that machine yet.</div>
            ) : null}
          </div>
        )}
      </Section>

      <Section step="2" title="To">
        <EndpointPills
          chosen={destination}
          machines={machines}
          online={isOnline}
          exclude={source}
          onPick={(ref) => {
            setDestination(ref);
            setPlan(null);
          }}
        />
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
        {plan?.destination_exists ? (
          <div className="mt-3 rounded-lg bg-warning/10 px-3 py-2 text-[12px] leading-relaxed text-warning">
            {destination?.kind === "this_computer" ? (
              <>
                <span className="mono">{folder}</span> already has this project. Its files are replaced by the ones from {machineName(source!)}, files that
                exist only here are deleted (excluded folders like .git and node_modules stay), and its data is replaced. Choose another folder to keep both.
              </>
            ) : (
              <>
                {machineName(destination!)} already has {name}: its folder becomes an exact copy (files only there are deleted) and its data is replaced.
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
              loading={planning || (!plan && sourceChosen && destination !== null)}
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
          <div className="text-[10px] uppercase tracking-[0.12em] text-ink-3">The source during the copy</div>
          <Radio
            value="stop"
            mode={mode}
            onChange={setMode}
            label={`Stop it for the data copy, then start it again${plan && !plan.source_running ? " (it is not running now)" : ""}`}
          />
          <Radio value="leave" mode={mode} onChange={setMode} label="Stop it and leave it stopped: a move, which also frees its ports" />
          <Radio value="keep" mode={mode} onChange={setMode} label="Keep it running: fast, but a database copied while it writes may be inconsistent" />
        </div>

        {toMachine && plan && plan.ports.length > 0 ? (
          <div className="mt-3 rounded-xl border border-line">
            <div className="border-b border-line bg-surface-2 px-3 py-1.5 text-[10px] uppercase tracking-[0.12em] text-ink-3">
              Ports on this computer afterwards
            </div>
            {plan.ports.map((port) => {
              const local = localFor(port);
              const taken = mode !== "leave" && busy.includes(local);
              return (
                <div key={port} className="flex items-center gap-2 px-3 py-1.5 text-[12px]">
                  <span className="tabular w-16 text-ink-2">:{port}</span>
                  <span className="text-ink-3">becomes localhost:</span>
                  <input
                    aria-label={`Port on this computer for ${port}`}
                    className="tabular w-16 rounded-md border border-line bg-surface-2 px-1.5 py-0.5 text-[12px] text-ink outline-none focus:border-accent disabled:opacity-50"
                    value={mode === "leave" ? port : local}
                    disabled={mode === "leave"}
                    onChange={(e) => setOverrides({ ...overrides, [port]: Number(e.target.value.replace(/\D/g, "")) || port })}
                  />
                  {taken ? (
                    <button
                      type="button"
                      className="whitespace-nowrap text-[11px] text-warning underline-offset-2 hover:underline"
                      onClick={() => setOverrides({ ...overrides, [port]: port + 1000 })}
                    >
                      in use on this computer, try {port + 1000}
                    </button>
                  ) : null}
                </div>
              );
            })}
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

      <DialogActions error={error}>
        <Button tone="ghost" onClick={close}>
          Cancel
        </Button>
        <Button
          tone="primary"
          onClick={() => void copy()}
          busy={working}
          disabled={planning || !plan || !request() || blocked || (!config && !data)}
          title={blocked ? "Pick other local ports or leave the source stopped" : undefined}
        >
          Copy to {destination ? machineName(destination) : "…"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function Section({ step, title, children }: { step: string; title: string; children: React.ReactNode }) {
  return (
    <section className="mt-5">
      <div className="flex items-baseline gap-2">
        <span className="tabular text-[11px] font-semibold text-accent">{step}</span>
        <h3 className="text-[13px] font-semibold text-ink">{title}</h3>
      </div>
      <div className="mt-2">{children}</div>
    </section>
  );
}

/** This computer and every machine; an offline machine is shown but cannot
 * be picked, so it does not look forgotten. */
function EndpointPills({
  chosen,
  machines,
  online,
  exclude,
  onPick,
}: {
  chosen: EndpointRef | null;
  machines: Array<{ id: string; name: string }>;
  online: (id: string) => boolean;
  exclude: EndpointRef | null;
  onPick: (ref: EndpointRef) => void;
}) {
  const options: Array<{ ref: EndpointRef; label: string; offline: boolean }> = [
    { ref: { kind: "this_computer" }, label: "This computer", offline: false },
    ...machines.map((m) => ({ ref: { kind: "machine", machine_id: m.id } as EndpointRef, label: m.name, offline: !online(m.id) })),
  ];
  return (
    <div className="flex flex-wrap gap-1.5">
      {options.map((option) => {
        const excluded = exclude !== null && sameEndpoint(option.ref, exclude);
        const selected = chosen !== null && sameEndpoint(option.ref, chosen);
        const unavailable = excluded || option.offline;
        return (
          <button
            key={option.label}
            type="button"
            disabled={unavailable}
            onClick={() => onPick(option.ref)}
            title={option.offline ? `${option.label} does not answer right now` : undefined}
            className={cx(
              "rounded-full border px-3 py-1 text-[12px] transition",
              selected ? "border-accent bg-accent-soft text-ink" : "border-line text-ink-2 hover:text-ink",
              unavailable && "opacity-40",
            )}
          >
            {option.label}
            {option.offline ? " (offline)" : ""}
          </button>
        );
      })}
    </div>
  );
}

function PickRow({
  selected,
  onClick,
  title,
  subtitle,
  chip,
  good,
  extra,
}: {
  selected: boolean;
  onClick: () => void;
  title: string;
  subtitle: string;
  chip?: string;
  good?: boolean;
  extra?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cx(
        "flex w-full items-center justify-between rounded-xl border px-3 py-2 text-left transition",
        selected ? "border-accent bg-accent-soft" : "border-line hover:bg-surface-2",
      )}
    >
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-ink">{title}</div>
        <div className="mono truncate text-[11px] text-ink-3">{subtitle}</div>
      </div>
      <div className="flex shrink-0 items-center gap-2 text-[11px] text-ink-3">
        {extra ? <span>{extra}</span> : null}
        {chip ? <Chip tone={good ? "good" : "neutral"}>{chip}</Chip> : null}
      </div>
    </button>
  );
}

function Radio({ value, mode, onChange, label }: { value: SourceMode; mode: SourceMode; onChange: (mode: SourceMode) => void; label: string }) {
  return (
    <label className="flex cursor-pointer items-start gap-2 text-ink-2">
      <input type="radio" name="source-mode" checked={mode === value} onChange={() => onChange(value)} className="mt-0.5 accent-accent" />
      <span>{label}</span>
    </label>
  );
}
