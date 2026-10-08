import type { ReactNode } from "react";

import { plural } from "../lib/format";
import type { EndpointRef, LocalProject, Stack } from "../lib/types";
import { SpinnerIcon } from "../ui/icons";
import { Chip, cx } from "../ui/primitives";
import { LooseContainers } from "./LooseContainers";

/** What happens to the source while its data is copied. */
export type SourceMode = "keep" | "stop" | "leave";

function sameEndpoint(a: EndpointRef, b: EndpointRef): boolean {
  if (a.kind === "this_computer" || b.kind === "this_computer") return a.kind === b.kind;
  return a.machine_id === b.machine_id;
}

/** A numbered step of the copy sheet. */
export function Section({ step, title, children }: { step: string; title: string; children: ReactNode }) {
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
export function EndpointPills({
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
  // Keyed by id: two machines may share a name.
  const options: Array<{ key: string; ref: EndpointRef; label: string; offline: boolean }> = [
    { key: "this_computer", ref: { kind: "this_computer" }, label: "This computer", offline: false },
    ...machines.map((m) => ({ key: m.id, ref: { kind: "machine", machine_id: m.id } as EndpointRef, label: m.name, offline: !online(m.id) })),
  ];
  return (
    <div className="flex flex-wrap gap-1.5">
      {options.map((option) => {
        const excluded = exclude !== null && sameEndpoint(option.ref, exclude);
        const selected = chosen !== null && sameEndpoint(option.ref, chosen);
        const unavailable = excluded || option.offline;
        return (
          <button
            key={option.key}
            type="button"
            disabled={unavailable}
            aria-pressed={selected}
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

/** What can be copied from the chosen source: this computer's compose
 * projects, or the machine's stacks. */
export function SourcePicker({
  source,
  projects,
  stacks,
  project,
  stackId,
  onProject,
  onStack,
}: {
  source: EndpointRef;
  /** null while this computer's Docker is being read. */
  projects: LocalProject[] | null;
  stacks: Stack[];
  project: LocalProject | null;
  stackId: string | null;
  onProject: (project: LocalProject) => void;
  onStack: (stack: Stack) => void;
}) {
  if (source.kind === "machine") {
    const onMachine = stacks.filter((s) => s.machine_id === source.machine_id);
    if (onMachine.length === 0) return <div className="mt-2 text-[12px] text-ink-3">No stacks on that machine yet.</div>;
    return (
      <div className="mt-2 space-y-1.5">
        {onMachine.map((s) => (
          <PickRow key={s.id} selected={stackId === s.id} onClick={() => onStack(s)} title={s.name} subtitle={s.project_dir} />
        ))}
      </div>
    );
  }
  if (projects === null) {
    return (
      <div className="mt-2 flex items-center gap-2 text-[12px] text-ink-3">
        <SpinnerIcon size={12} /> reading this computer's Docker
      </div>
    );
  }
  return (
    <div className="mt-2 space-y-1.5">
      {projects.length === 0 ? <div className="text-[12px] text-ink-3">Docker on this computer has no compose projects right now.</div> : null}
      {projects.map((p) => (
        <PickRow
          key={p.name}
          selected={project?.name === p.name}
          onClick={() => onProject(p)}
          title={p.name}
          subtitle={p.project_dir}
          chip={p.status}
          good={p.status.startsWith("running")}
          extra={`${plural(p.volumes.length, "volume")} · ${plural(p.ports.length, "port")}`}
        />
      ))}
      <LooseContainers />
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
      aria-pressed={selected}
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

/** One way to treat the source during the copy. */
export function SourceModeRadio({
  value,
  mode,
  onChange,
  label,
}: {
  value: SourceMode;
  mode: SourceMode;
  onChange: (mode: SourceMode) => void;
  label: string;
}) {
  return (
    <label className="flex cursor-pointer items-start gap-2 text-ink-2">
      <input type="radio" name="source-mode" checked={mode === value} onChange={() => onChange(value)} className="mt-0.5 accent-accent" />
      <span>{label}</span>
    </label>
  );
}
