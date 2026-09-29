import type { BindMount } from "../lib/types";
import { Chip, Toggle } from "../ui/primitives";

/** The folders inside the project that containers mount, each with the
 * choice to copy it from this computer or leave the machine's own alone.
 * The sync deletes on the machine what this computer lacks, so a database's
 * data folder must never be copied over it; one that only the machine has
 * starts out left alone. */
export function BindMounts({ binds, skipped, onSkip }: { binds: BindMount[]; skipped: string[]; onSkip: (path: string, skip: boolean) => void }) {
  if (binds.length === 0) return null;
  return (
    <div className="mt-4 rounded-xl border border-line">
      <div className="border-b border-line bg-surface-2 px-3 py-1.5 text-[10px] uppercase tracking-[0.12em] text-ink-3">Folders the containers mount</div>
      <ul className="divide-y divide-line">
        {binds.map((bind) => {
          const copied = !skipped.includes(bind.path);
          return (
            <li key={bind.path} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[12px]">
              <span className="mono min-w-0 flex-1 truncate text-ink" title={bind.path}>
                {bind.path}
              </span>
              <span className="text-ink-3">{bind.services.join(", ")}</span>
              {bind.read_only ? <Chip tone="neutral">read-only</Chip> : null}
              <Toggle
                checked={copied}
                onChange={(on) => onSkip(bind.path, !on)}
                label={copied ? "copied from this computer" : "left as it is on the machine"}
              />
              {!bind.exists_here ? <span className="basis-full text-[11px] text-ink-3">Not on this computer, so the machine keeps its own.</span> : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** What an unticked folder becomes in the stack's excludes: anchored to the
 * project root, as rsync reads a leading slash, so `data` alone does not
 * also skip an `app/data`. */
export function excludeFor(bind: BindMount): string {
  return `/${bind.path}`;
}
