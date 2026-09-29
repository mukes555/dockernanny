import type { ContainerData, VolumePlan } from "../lib/types";
import { SpinnerIcon } from "../ui/icons";
import { cx } from "../ui/primitives";

/** `service:path`, the key a ticked container path is remembered by. */
export function selectionKey(service: string, path: string): string {
  return `${service}:${path}`;
}

export function splitSelectionKey(key: string): { service: string; path: string } {
  const separator = key.indexOf(":");
  return { service: key.slice(0, separator), path: key.slice(separator + 1) };
}

/** What travels by default: every anonymous volume, and the changed folders
 * that look like data. */
export function defaultSelection(data: ContainerData[]): Set<string> {
  const keys = new Set<string>();
  for (const container of data) {
    for (const volume of container.anonymous_volumes) keys.add(selectionKey(container.service, volume.destination));
    for (const path of container.changed_paths) if (path.suggested) keys.add(selectionKey(container.service, path.path));
  }
  return keys;
}

/** The user's ticks that the new plan still offers; paths that are gone drop out. */
export function keepOffered(ticked: Set<string>, data: ContainerData[]): Set<string> {
  const offered = new Set<string>();
  for (const container of data) {
    for (const volume of container.anonymous_volumes) offered.add(selectionKey(container.service, volume.destination));
    for (const path of container.changed_paths) offered.add(selectionKey(container.service, path.path));
  }
  return new Set([...ticked].filter((key) => offered.has(key)));
}

/** The box that lists everything a data copy carries: the named volumes,
 * and per container the data the image keeps for itself or the container
 * wrote into its own layer. */
export function CopyData({
  volumes,
  containers,
  loading,
  selected,
  onToggle,
}: {
  volumes: VolumePlan[];
  containers: ContainerData[];
  loading: boolean;
  selected: Set<string>;
  onToggle: (key: string, on: boolean) => void;
}) {
  const withData = containers.filter((c) => c.anonymous_volumes.length > 0 || c.changed_paths.length > 0);
  return (
    <div className="rounded-xl border border-line">
      <div className="border-b border-line bg-surface-2 px-3 py-1.5 text-[10px] uppercase tracking-[0.12em] text-ink-3">Data that travels</div>
      {loading ? (
        <div className="flex items-center gap-2 px-3 py-2 text-[12px] text-ink-3">
          <SpinnerIcon size={12} /> looking at the source
        </div>
      ) : null}
      {!loading && volumes.length === 0 ? <div className="px-3 py-2 text-[12px] text-ink-3">no named volumes</div> : null}
      {volumes.map((v) => (
        <div key={v.name} className="flex items-center justify-between gap-3 px-3 py-1.5 text-[12px]">
          <span className="mono truncate text-ink">{v.name}</span>
          <span className="shrink-0 text-[11px] text-ink-3">
            {v.external ? "external, added to" : `replaces ${v.destination_name}`} · <span className="tabular">{v.size}</span>
          </span>
        </div>
      ))}

      <div className="border-t border-line bg-surface-2/60 px-3 py-1.5 text-[10px] uppercase tracking-[0.12em] text-ink-3">Inside the containers</div>
      {!loading && withData.length === 0 ? <div className="px-3 py-2 text-[12px] text-ink-3">nothing kept outside the named volumes</div> : null}
      {withData.map((container) => (
        <div key={container.service}>
          {container.anonymous_volumes.map((volume) => (
            <Row
              key={volume.destination}
              checked={selected.has(selectionKey(container.service, volume.destination))}
              onChange={(on) => onToggle(selectionKey(container.service, volume.destination), on)}
              service={container.service}
              path={volume.destination}
              detail={`volume the image keeps for itself, ${volume.size}`}
            />
          ))}
          {container.changed_paths.map((changed) => (
            <Row
              key={changed.path}
              checked={selected.has(selectionKey(container.service, changed.path))}
              onChange={(on) => onToggle(selectionKey(container.service, changed.path), on)}
              service={container.service}
              path={changed.path}
              detail={`changed inside the container, ${changed.entries} ${changed.entries === 1 ? "entry" : "entries"}`}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function Row({
  checked,
  onChange,
  service,
  path,
  detail,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  service: string;
  path: string;
  detail: string;
}) {
  return (
    <label className={cx("flex cursor-pointer items-center gap-2.5 px-3 py-1.5 text-[12px] hover:bg-surface-2/60", checked ? "text-ink" : "text-ink-2")}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="accent-accent" />
      <span className="w-16 shrink-0 truncate font-medium">{service}</span>
      <span className="mono min-w-0 flex-1 truncate">{path}</span>
      <span className="shrink-0 text-[11px] text-ink-3">{detail}</span>
    </label>
  );
}
