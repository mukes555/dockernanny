import type { ReactNode } from "react";

import type { Probe } from "../lib/types";
import { BatteryPill, gigabytesOf, OsGlyph, uptimeText } from "./Badges";
import { gigabytes, loadPercent, memoryPercent, Meter } from "./Meter";

/** What the probe found on a computer, laid out the same way for a machine's
 * page and for this computer's page: three gauges, then the facts. */
export function ProbeFacts({ probe, children }: { probe: Probe; children?: ReactNode }) {
  const diskUsed = probe.disk_total_bytes - probe.disk_free_bytes;
  const diskPercent = probe.disk_total_bytes > 0 ? (diskUsed / probe.disk_total_bytes) * 100 : 0;
  const cores = probe.cpus > 0 ? `${probe.cpus} ${probe.cpus === 1 ? "core" : "cores"}` : null;
  const processor = [probe.cpu_model, cores].filter(Boolean).join(" · ");
  const docker = probe.docker_version ? `${probe.docker_version} · ${probe.containers_running} running` : "not found";

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-1 gap-x-6 gap-y-2 md:grid-cols-3">
        <Meter label="load" value={loadPercent(probe.load1, probe.cpus)} text={`${probe.load1.toFixed(1)} / ${probe.cpus} cpus`} size="md" />
        {probe.mem_total_mb > 0 ? <Meter label="ram" value={memoryPercent(probe.mem_used_mb, probe.mem_total_mb)} text={`${gigabytes(probe.mem_used_mb)} / ${gigabytes(probe.mem_total_mb)} GB`} size="md" /> : null}
        {probe.disk_total_bytes > 0 ? <Meter label="disk" value={diskPercent} text={`${gigabytesOf(probe.disk_free_bytes)} free`} size="md" /> : null}
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-[12px] md:grid-cols-3">
        <Fact label="System">
          <OsGlyph os={probe.os} size={13} className="shrink-0 text-ink-3" />
          <span className="truncate" title={probe.os ?? undefined}>
            {probe.os ?? "unknown"}
          </span>
        </Fact>
        <Fact label="Processor">{processor || "unknown"}</Fact>
        <Fact label="Up for">{probe.uptime_s > 0 ? uptimeText(probe.uptime_s).replace(/^up /, "") : "unknown"}</Fact>
        <Fact label="Battery">{probe.battery ? <BatteryPill battery={probe.battery} /> : <span className="text-ink-3">none, mains power</span>}</Fact>
        <Fact label="Docker">{docker}</Fact>
        {children}
      </dl>
    </div>
  );
}

export function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-[10px] uppercase tracking-[0.12em] text-ink-3">{label}</dt>
      <dd className="mt-0.5 flex items-center gap-1.5 text-ink">{children}</dd>
    </div>
  );
}
