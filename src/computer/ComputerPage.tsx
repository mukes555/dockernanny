import { useEffect, useState } from "react";

import { SharingSections } from "../host/SharingSections";
import { api, errorMessage } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import type { LocalProject } from "../lib/types";
import { useStore } from "../state/store";
import { OsGlyph } from "../ui/Badges";
import { ArrowLeftIcon, ExternalIcon, RefreshIcon, SpinnerIcon } from "../ui/icons";
import { Button, Card, Chip, Eyebrow } from "../ui/primitives";
import { ProbeFacts } from "../ui/ProbeFacts";
import { Readiness } from "./Readiness";

/** This computer as a place: who it is and how it is doing, what its own
 * Docker runs (the starting point for sending a project to a machine), and
 * the sharing role underneath. */
export function ComputerPage() {
  const info = useStore((state) => state.computerInfo);
  const computerName = useStore((state) => state.computerName);
  const loadComputerInfo = useStore((state) => state.loadComputerInfo);
  const setView = useStore((state) => state.setView);
  const settings = useStore((state) => state.settings);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadComputerInfo().catch((err) => setError(errorMessage(err)));
  }, [loadComputerInfo]);

  return (
    <div className="mx-auto max-w-3xl space-y-5 pb-10">
      <button type="button" onClick={() => setView("stacks")} className="inline-flex items-center gap-1 text-[12px] text-ink-3 transition hover:text-ink">
        <ArrowLeftIcon size={13} /> Back
      </button>
      <header className="rounded-2xl border border-line bg-surface p-5">
        <Eyebrow>This computer</Eyebrow>
        <div className="mt-1 flex items-center gap-2.5">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-accent">
            <OsGlyph os={info?.probe.os} size={18} />
          </span>
          <h1 className="truncate text-xl font-semibold tracking-tight text-ink">{computerName || "this computer"}</h1>
        </div>
        {info ? (
          <div className="mono mt-1 text-[12px] text-ink-3">
            {info.user}@{info.probe.hostname ?? computerName}
          </div>
        ) : null}
        <div className="mt-4">
          {info ? (
            <ProbeFacts probe={info.probe} />
          ) : error ? null : (
            <div className="flex items-center gap-2 text-[13px] text-ink-2">
              <SpinnerIcon /> Looking at this computer…
            </div>
          )}
        </div>
        {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}
      </header>

      {settings?.use_machines ? <Readiness /> : null}
      <DockerHere />
      <section id="sharing" className="scroll-mt-4">
        <SharingSections />
      </section>
    </div>
  );
}

/** The compose projects this computer's own Docker runs. Each one can be
 * sent to a machine from here; that is the "sync from my computer" story. */
function DockerHere() {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const settings = useStore((state) => state.settings);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const [projects, setProjects] = useState<LocalProject[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const machines = visibleMachines(allMachines, computerInfo);
  const canCopy = (settings?.use_machines ?? true) && machines.length > 0;

  const refresh = () => {
    setProjects(null);
    setError(null);
    api
      .localProjects()
      .then(setProjects)
      .catch((err) => setError(errorMessage(err)));
  };
  useEffect(refresh, []);

  return (
    <Card
      title="Docker on this computer"
      description="Compose projects running in the local Docker. Copy one to a machine to run it there instead."
      actions={
        <Button size="sm" tone="ghost" onClick={refresh} disabled={projects === null} aria-label="Refresh">
          <RefreshIcon />
        </Button>
      }
    >
      {error ? <DockerMissing detail={error} /> : null}
      {projects === null && !error ? (
        <div className="flex items-center gap-2 text-[13px] text-ink-2">
          <SpinnerIcon /> reading this computer's Docker
        </div>
      ) : null}
      {projects?.length === 0 ? (
        <div className="flex flex-wrap items-center justify-between gap-3 text-[13px] text-ink-2">
          <span>Nothing runs in Docker here right now. Start a compose project, or bring a stack back from a machine.</span>
          <Button onClick={() => setCopyOpen({ open: true, toThisComputer: true })} disabled={!canCopy}>
            Copy a stack here
          </Button>
        </div>
      ) : null}
      {projects && projects.length > 0 ? (
        <div className="space-y-2">
          {projects.map((project) => (
            <ProjectRow key={project.name} project={project} canCopy={canCopy} onCopy={() => setCopyOpen({ open: true, sourceProject: project.name })} />
          ))}
        </div>
      ) : null}
    </Card>
  );
}

/** `docker compose ls` failing here almost always means Docker is not
 * installed or not started, which is fine: only copies from this computer need it. */
function DockerMissing({ detail }: { detail: string }) {
  return (
    <div className="rounded-xl border border-dashed border-hairline px-4 py-3">
      <div className="text-[13px] font-medium text-ink">Docker isn't installed or running here</div>
      <p className="mt-1 text-[12px] leading-relaxed text-ink-2">
        That is fine for running stacks on machines. It is only needed to copy this computer's own projects, and to preview a dropped compose file. Start Docker Desktop, OrbStack or Docker Engine, then refresh.
      </p>
      <details className="mt-2 text-[11px] text-ink-3">
        <summary className="cursor-pointer">What Docker said</summary>
        <pre className="mono selectable mt-1 whitespace-pre-wrap">{detail}</pre>
      </details>
    </div>
  );
}

function ProjectRow({ project, canCopy, onCopy }: { project: LocalProject; canCopy: boolean; onCopy: () => void }) {
  const running = project.status.startsWith("running");
  const volumes = project.volumes.length === 1 ? "1 volume" : `${project.volumes.length} volumes`;
  return (
    <div className="rounded-xl border border-line bg-surface-2/40 px-3 py-2.5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-[13px] font-medium text-ink">{project.name}</span>
            <Chip tone={running ? "good" : "neutral"}>{project.status}</Chip>
          </div>
          <div className="mono mt-0.5 truncate text-[11px] text-ink-3" title={project.config_file}>
            {project.project_dir}
          </div>
        </div>
        <Button size="sm" onClick={onCopy} disabled={!canCopy} title={canCopy ? "Copy this project's config and data to a machine" : "Add a machine first"}>
          Copy to…
        </Button>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-3">
        <span>{volumes}</span>
        {project.ports.map((port) => (
          <button key={port} type="button" className="tabular inline-flex items-center gap-1 text-ink-2 underline-offset-2 hover:text-ink hover:underline" onClick={() => void api.openLocal(port)} title={`Open localhost:${port}`}>
            :{port} <ExternalIcon size={10} />
          </button>
        ))}
        {project.warnings.map((warning) => (
          <span key={warning} className="text-warning" title={warning}>
            note: {warning}
          </span>
        ))}
      </div>
    </div>
  );
}
