import { useEffect, useState } from "react";

import { SharingSections } from "../host/SharingSections";
import { plural } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { visibleMachines } from "../lib/machines";
import type { LocalProject } from "../lib/types";
import { LooseContainers } from "../stacks/LooseContainers";
import { useStore } from "../state/store";
import { OsGlyph } from "../ui/Badges";
import { ExternalIcon, RefreshIcon, SpinnerIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { Button, Card, Chip, ErrorLine, Inset } from "../ui/primitives";
import { ProbeFacts } from "../ui/ProbeFacts";
import { useLoaded } from "../ui/useLoaded";
import { Readiness } from "./Readiness";

/** This computer as a place, in three parts: who it is and whether it can
 * reach machines, what its own Docker runs (the starting point for sending
 * a project to a machine), and the sharing role. */
export function ComputerPage() {
  const info = useStore((state) => state.computerInfo);
  const computerName = useStore((state) => state.computerName);
  const loadComputerInfo = useStore((state) => state.loadComputerInfo);
  const settings = useStore((state) => state.settings);
  const tab = useStore((state) => state.computerTab);
  const openComputer = useStore((state) => state.openComputer);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadComputerInfo().catch((err) => setError(errorMessage(err)));
  }, [loadComputerInfo]);

  const who = info ? `${info.user}@${info.probe.hostname ?? computerName}` : "This computer";
  const roles = [settings?.use_machines ? "uses other machines" : null, settings?.share_this_computer ? "shared with others" : null]
    .filter(Boolean)
    .join(" and ");

  return (
    <Page
      title={
        <>
          <OsGlyph os={info?.probe.os} size={18} className="shrink-0 text-accent" />
          <span className="truncate">{computerName || "This computer"}</span>
        </>
      }
      summary={
        <>
          <span className="mono">{who}</span>
          {roles ? ` · ${roles}` : ""}
        </>
      }
      tabs={{
        value: tab,
        onChange: openComputer,
        items: [
          { id: "overview", label: "Overview" },
          { id: "docker", label: "Docker here" },
          { id: "sharing", label: "Sharing" },
        ],
      }}
    >
      {tab === "overview" ? (
        <>
          <Card title="This computer">
            {info ? <ProbeFacts probe={info.probe} /> : null}
            {!info && !error ? (
              <div className="flex items-center gap-2 text-[13px] text-ink-2">
                <SpinnerIcon /> Looking at this computer…
              </div>
            ) : null}
            <ErrorLine error={error} className="mt-3" />
          </Card>
          {settings?.use_machines ? <Readiness /> : null}
        </>
      ) : null}
      {tab === "docker" ? <DockerHere /> : null}
      {tab === "sharing" ? <SharingSections /> : null}
    </Page>
  );
}

/** The compose projects this computer's own Docker runs. Each one can be
 * sent to a machine from here; that is the "sync from my computer" story. */
function DockerHere() {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const settings = useStore((state) => state.settings);
  const setCopyOpen = useStore((state) => state.setCopyOpen);
  const local = useLoaded(api.localProjects);
  const machines = visibleMachines(allMachines, computerInfo);
  const canCopy = (settings?.use_machines ?? true) && machines.length > 0;
  // A failed read shows why; the last list stays hidden until a read works again.
  const projects = local.error ? null : local.data;

  return (
    <Card
      title="Docker on this computer"
      description="Compose projects running in the local Docker. Copy one to a machine to run it there instead."
      actions={
        <Button size="sm" tone="ghost" onClick={() => void local.reload()} busy={local.loading} aria-label="Refresh">
          <RefreshIcon />
        </Button>
      }
    >
      {local.error ? <DockerMissing detail={local.error} /> : null}
      {local.loading && projects === null && !local.error ? (
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
      {projects ? (
        <div className="mt-3">
          <LooseContainers />
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
        That is fine for running stacks on machines. It is only needed to copy this computer's own projects, and to preview a dropped compose file. Start Docker
        Desktop, OrbStack or Docker Engine, then refresh.
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
  const volumes = plural(project.volumes.length, "volume");
  return (
    <Inset>
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
          <button
            key={port}
            type="button"
            className="tabular inline-flex items-center gap-1 text-ink-2 underline-offset-2 hover:text-ink hover:underline"
            onClick={() => void api.openLocal(port)}
            title={`Open localhost:${port}`}
          >
            :{port} <ExternalIcon size={10} />
          </button>
        ))}
        {project.warnings.map((warning) => (
          <span key={warning} className="text-warning" title={warning}>
            note: {warning}
          </span>
        ))}
      </div>
    </Inset>
  );
}
