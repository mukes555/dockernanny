import type { DoctorRow } from "../lib/types";
import { CheckIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { CodeBlock } from "../ui/primitives";

/** The five things a machine needs, in the order the doctor checks them. */
export const CHECKS: Array<{ key: string; label: string }> = [
  { key: "ssh", label: "SSH" },
  { key: "docker", label: "Docker" },
  { key: "compose", label: "Compose" },
  { key: "rsync", label: "rsync" },
  { key: "host", label: "Host" },
];

/** The doctor's results as they arrive: a spinner while a check runs, the
 * fix under a failed one, "skipped" for the rest once ssh failed. The same
 * rows show this computer's readiness, whose fixes are sentences, not commands. */
export function DoctorRows({ rows, checking, checks = CHECKS, fixesAreCommands = true }: { rows: DoctorRow[]; checking: boolean; checks?: Array<{ key: string; label: string }>; fixesAreCommands?: boolean }) {
  // A machine that does not answer ssh cannot be checked further; this computer has no such gate.
  const sshOk = rows.some((row) => row.key === "ssh" && row.ok);
  const skipped = fixesAreCommands && !checking && rows.length > 0 && !sshOk;
  return (
    <div className="space-y-1.5">
      {checks.map((check) => (
        <CheckRow key={check.key} label={check.label} row={rows.find((row) => row.key === check.key)} pending={checking} skipped={skipped} fixIsCommand={fixesAreCommands} />
      ))}
    </div>
  );
}

function CheckRow({ label, row, pending, skipped, fixIsCommand }: { label: string; row?: DoctorRow; pending: boolean; skipped: boolean; fixIsCommand: boolean }) {
  const icon = row ? row.ok ? <CheckIcon className="text-good" /> : <XIcon className="text-critical" /> : pending ? <SpinnerIcon className="text-ink-3" /> : <span className="inline-block h-3.5 w-3.5" />;
  const detail = row ? row.detail : skipped ? "skipped" : pending ? "checking" : "";
  return (
    <div className="rounded-lg border border-line bg-surface-2/60 px-3 py-2">
      <div className="flex items-center gap-2.5 text-[12px]">
        <span className="flex w-4 justify-center">{icon}</span>
        <span className="w-24 shrink-0 font-medium text-ink">{label}</span>
        <span className="min-w-0 flex-1 truncate text-ink-2" title={detail}>
          {detail}
        </span>
      </div>
      {row?.fix ? <div className="mt-2 ml-6">{fixIsCommand ? <CodeBlock code={row.fix} /> : <p className="text-[12px] leading-relaxed text-ink-2">{row.fix}</p>}</div> : null}
    </div>
  );
}
