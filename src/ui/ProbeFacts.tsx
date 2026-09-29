import type { ReactNode } from "react";

import { byteSize, memoryGb, plural, span } from "../lib/format";
import type { Probe } from "../lib/types";
import { BatteryPill, OsGlyph } from "./Badges";
import { loadPercent, memoryPercent, Meter } from "./Meter";
import { EYEBROW } from "./primitives";

/** What the probe found on a computer, laid out the same way for a machine's
 * page and for this computer's page: three gauges, then the facts. */
export function ProbeFacts({ probe, children }: { probe: Probe; children?: ReactNode }) {
  const diskUsed = probe.disk_total_bytes - probe.disk_free_bytes;
  const diskPercent = probe.disk_total_bytes > 0 ? (diskUsed / probe.disk_total_bytes) * 100 : 0;
  const cores = probe.cpus > 0 ? plural(probe.cpus, "core") : null;
  const processor = [probe.cpu_model, cores].filter(Boolean).join(" · ");
  const docker = probe.docker_version ? `${probe.docker_version} · ${probe.containers_running} running` : "not found";

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-1 gap-x-6 gap-y-2 md:grid-cols-3">
        <Meter label="load" value={loadPercent(probe.load1, probe.cpus)} text={`${probe.load1.toFixed(1)} / ${probe.cpus} cpus`} size="md" />
        {probe.mem_total_mb > 0 ? (
          <Meter
            label="ram"
            value={memoryPercent(probe.mem_used_mb, probe.mem_total_mb)}
            text={`${memoryGb(probe.mem_used_mb)} / ${memoryGb(probe.mem_total_mb)} GB`}
            size="md"
          />
        ) : null}
        {probe.disk_total_bytes > 0 ? <Meter label="disk" value={diskPercent} text={`${byteSize(probe.disk_free_bytes)} free`} size="md" /> : null}
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-[12px] md:grid-cols-3">
        <Fact label="System">
          <OsGlyph os={probe.os} size={13} className="shrink-0 text-ink-3" />
          <span className="truncate" title={probe.os ?? undefined}>
            {probe.os ?? "unknown"}
          </span>
        </Fact>
        <Fact label="Processor">{processor || "unknown"}</Fact>
        <Fact label="Up for">{probe.uptime_s > 0 ? span(probe.uptime_s * 1000) : "unknown"}</Fact>
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
      <dt className={EYEBROW}>{label}</dt>
      <dd className="mt-0.5 flex items-center gap-1.5 text-ink">{children}</dd>
    </div>
  );
}
